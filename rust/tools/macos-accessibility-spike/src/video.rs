//! An `NSView` that shows libmpv video through the render API: a
//! `CAOpenGLLayer` whose drawing calls [`MpvRenderHost::draw`].

#![allow(unsafe_code, deprecated)]

use std::ffi::{c_char, c_void};
use std::ptr::NonNull;
use std::sync::LazyLock;

use apricot_playback::MpvRenderHost;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::NSView;
use objc2_core_foundation::CFTimeInterval;
use objc2_core_video::CVTimeStamp;
use objc2_foundation::{NSObject, NSRect};
use objc2_open_gl::{
    CGLChoosePixelFormat, CGLContextObj, CGLOpenGLProfile, CGLPixelFormatAttribute,
    CGLPixelFormatObj,
};

type GLint = i32;
use objc2_quartz_core::{CALayer, CAOpenGLLayer};

static FRAMES_DRAWN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many video frames libmpv has drawn into a surface.
pub fn frames_drawn() -> u64 {
    FRAMES_DRAWN.load(std::sync::atomic::Ordering::Relaxed)
}

const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_VIEWPORT: u32 = 0x0BA2;

type GlGetIntegerv = unsafe extern "C" fn(u32, *mut GLint);

/// The system OpenGL framework, for libmpv's function lookups.
static OPENGL: LazyLock<Option<libloading::Library>> = LazyLock::new(|| {
    // SAFETY: loading Apple's OpenGL framework runs no foreign initializers
    // beyond the framework's own.
    unsafe { libloading::Library::new("/System/Library/Frameworks/OpenGL.framework/OpenGL").ok() }
});

unsafe extern "C" fn get_proc_address(_context: *mut c_void, name: *const c_char) -> *mut c_void {
    let Some(library) = OPENGL.as_ref() else {
        return std::ptr::null_mut();
    };
    // SAFETY: libmpv passes a NUL-terminated GL function name; the symbol is
    // only returned as an address.
    unsafe {
        let name = std::ffi::CStr::from_ptr(name).to_bytes_with_nul();
        library
            .get::<*mut c_void>(name)
            .map_or(std::ptr::null_mut(), |symbol| *symbol)
    }
}

fn gl_get_integers(parameter: u32, values: &mut [GLint]) {
    let Some(get) = OPENGL
        .as_ref()
        .and_then(|library| unsafe { library.get::<GlGetIntegerv>(b"glGetIntegerv\0").ok() })
    else {
        return;
    };
    // SAFETY: `values` is large enough for the queried parameter.
    unsafe { get(parameter, values.as_mut_ptr()) };
}

pub struct VideoLayerIvars {
    host: MpvRenderHost,
}

define_class!(
    // SAFETY: CAOpenGLLayer may be subclassed; no Drop impl.
    #[unsafe(super(CAOpenGLLayer, CALayer, NSObject))]
    #[name = "ApricotVideoLayer"]
    #[ivars = VideoLayerIvars]
    pub struct VideoLayer;

    impl VideoLayer {
        #[unsafe(method(copyCGLPixelFormatForDisplayMask:))]
        fn pixel_format(&self, mask: u32) -> CGLPixelFormatObj {
            let attributes = [
                CGLPixelFormatAttribute::CGLPFAOpenGLProfile,
                CGLPixelFormatAttribute(CGLOpenGLProfile::CGLOGLPVersion_3_2_Core.0),
                CGLPixelFormatAttribute::CGLPFADoubleBuffer,
                CGLPixelFormatAttribute::CGLPFAAccelerated,
                CGLPixelFormatAttribute::CGLPFAAllowOfflineRenderers,
                CGLPixelFormatAttribute::CGLPFADisplayMask,
                CGLPixelFormatAttribute(mask),
                CGLPixelFormatAttribute(0),
            ];
            let mut format: CGLPixelFormatObj = std::ptr::null_mut();
            let mut count: GLint = 0;
            // SAFETY: the attribute list is zero-terminated and the outputs
            // are valid.
            unsafe {
                CGLChoosePixelFormat(
                    NonNull::from(&attributes[0]),
                    NonNull::from(&mut format),
                    NonNull::from(&mut count),
                );
            }
            format
        }

        #[unsafe(method(canDrawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn can_draw(
            &self,
            _context: CGLContextObj,
            _format: CGLPixelFormatObj,
            _time: CFTimeInterval,
            _timestamp: *const CVTimeStamp,
        ) -> bool {
            true
        }

        #[unsafe(method(drawInCGLContext:pixelFormat:forLayerTime:displayTime:))]
        fn draw(
            &self,
            context: CGLContextObj,
            format: CGLPixelFormatObj,
            time: CFTimeInterval,
            timestamp: *const CVTimeStamp,
        ) {
            let mut fbo = [0];
            gl_get_integers(GL_FRAMEBUFFER_BINDING, &mut fbo);
            let mut viewport = [0; 4];
            gl_get_integers(GL_VIEWPORT, &mut viewport);
            // SAFETY: CAOpenGLLayer made `context` current on this (main)
            // thread for this call.
            let drawn = unsafe {
                self.ivars().host.draw(
                    get_proc_address,
                    std::ptr::null_mut(),
                    fbo[0],
                    viewport[2],
                    viewport[3],
                )
            };
            // SAFETY: the superclass flushes the context.
            unsafe {
                let _: () = msg_send![
                    super(self),
                    drawInCGLContext: context,
                    pixelFormat: format,
                    forLayerTime: time,
                    displayTime: timestamp
                ];
            }
            if drawn {
                FRAMES_DRAWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.ivars().host.report_swap();
            }
        }
    }
);

/// Asks the main thread to redraw the layer; called by libmpv from its
/// thread.
unsafe extern "C" fn request_redraw(layer: *mut c_void) {
    let layer = layer as usize;
    dispatch2::DispatchQueue::main().exec_async(move || {
        // SAFETY: the layer lives as long as the view that owns the host.
        let layer = unsafe { &*(layer as *const VideoLayer) };
        layer.setNeedsDisplay();
    });
}

/// Creates the video view and the render host libmpv draws into.
pub fn video_view(mtm: MainThreadMarker, frame: NSRect) -> (Retained<NSView>, MpvRenderHost) {
    let host = MpvRenderHost::new();
    let layer = VideoLayer::alloc().set_ivars(VideoLayerIvars { host: host.clone() });
    // SAFETY: `init` of CAOpenGLLayer.
    let layer: Retained<VideoLayer> = unsafe { msg_send![super(layer), init] };
    layer.setAsynchronous(false);
    layer.setNeedsDisplayOnBoundsChange(true);
    let pointer: *const VideoLayer = &raw const *layer;
    // SAFETY: `request_redraw` only schedules main-thread work and the layer
    // is kept alive by the view below for as long as the host exists.
    unsafe { host.set_update_callback(request_redraw, pointer.cast_mut().cast()) };
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    let _: &AnyObject = layer.as_ref();
    (view, host)
}
