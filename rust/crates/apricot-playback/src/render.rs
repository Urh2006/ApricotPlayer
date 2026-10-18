//! libmpv's OpenGL render API (`vo=libmpv`) for platforms where libmpv cannot
//! draw into a foreign window through `wid`, which is macOS.
//!
//! The UI owns an [`MpvRenderHost`] and draws from its own OpenGL context; the
//! engine attaches its client to the host when it starts and detaches before
//! it destroys the client. libmpv requires that the render context is created
//! and freed on the drawing thread and freed before the client is destroyed,
//! so detaching waits (bounded) until the drawing thread has freed it.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    collections::HashMap,
    ffi::{c_char, c_int, c_void},
    path::Path,
    ptr,
    sync::{Arc, Condvar, LazyLock, Mutex, Weak, atomic::AtomicU64},
    time::Duration,
};

use libloading::Library;

use crate::PlaybackError;

const MPV_RENDER_PARAM_INVALID: c_int = 0;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;
const MPV_RENDER_UPDATE_FRAME: u64 = 1;

/// The OpenGL `get_proc_address` callback libmpv uses to find GL functions.
pub type GetProcAddress = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
/// Called from an mpv thread when a new frame should be drawn.
pub type UpdateCallback = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: Option<GetProcAddress>,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

type ContextCreate = unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int;
type ContextSetUpdateCallback =
    unsafe extern "C" fn(*mut c_void, Option<UpdateCallback>, *mut c_void);
type ContextUpdate = unsafe extern "C" fn(*mut c_void) -> u64;
type ContextRender = unsafe extern "C" fn(*mut c_void, *mut RenderParam) -> c_int;
type ContextReportSwap = unsafe extern "C" fn(*mut c_void);
type ContextFree = unsafe extern "C" fn(*mut c_void);

struct RenderApi {
    _library: Library,
    create: ContextCreate,
    set_update_callback: ContextSetUpdateCallback,
    update: ContextUpdate,
    render: ContextRender,
    report_swap: ContextReportSwap,
    free: ContextFree,
}

impl RenderApi {
    unsafe fn load(path: &Path) -> Result<Self, PlaybackError> {
        let library = Library::new(path).map_err(|error| {
            PlaybackError::Operation(format!(
                "could not load libmpv from {}: {error}",
                path.display()
            ))
        })?;
        let symbol = |name: &[u8]| {
            library
                .get::<*mut c_void>(name)
                .map(|symbol| *symbol)
                .map_err(|error| {
                    PlaybackError::Operation(format!("libmpv render API is missing: {error}"))
                })
        };
        let create = symbol(b"mpv_render_context_create\0")?;
        let set_update_callback = symbol(b"mpv_render_context_set_update_callback\0")?;
        let update = symbol(b"mpv_render_context_update\0")?;
        let render = symbol(b"mpv_render_context_render\0")?;
        let report_swap = symbol(b"mpv_render_context_report_swap\0")?;
        let free = symbol(b"mpv_render_context_free\0")?;
        // SAFETY: the symbols are libmpv's exported render functions with
        // the signatures declared in `mpv/render.h`.
        Ok(Self {
            create: std::mem::transmute::<*mut c_void, ContextCreate>(create),
            set_update_callback: std::mem::transmute::<*mut c_void, ContextSetUpdateCallback>(
                set_update_callback,
            ),
            update: std::mem::transmute::<*mut c_void, ContextUpdate>(update),
            render: std::mem::transmute::<*mut c_void, ContextRender>(render),
            report_swap: std::mem::transmute::<*mut c_void, ContextReportSwap>(report_swap),
            free: std::mem::transmute::<*mut c_void, ContextFree>(free),
            _library: library,
        })
    }
}

/// What the engine has attached and what the drawing thread has created.
#[derive(Default)]
struct RenderState {
    api: Option<Arc<RenderApi>>,
    /// The attached mpv client (`mpv_handle*`).
    client: usize,
    /// The render context (`mpv_render_context*`), created on the drawing
    /// thread.
    context: usize,
    /// The engine waits for the drawing thread to free the context.
    detaching: bool,
    creation_failed: bool,
    setup_cancelled: bool,
    update_callback: Option<(UpdateCallback, usize)>,
}

struct RenderShared {
    id: u64,
    state: Mutex<RenderState>,
    freed: Condvar,
}

// SAFETY: the raw pointers in `RenderState` are only used under the mutex,
// and libmpv's render functions are documented as callable from any one
// drawing thread.
unsafe impl Send for RenderShared {}
// SAFETY: as above.
unsafe impl Sync for RenderShared {}

static NEXT_HOST_ID: AtomicU64 = AtomicU64::new(1);
static HOSTS: LazyLock<Mutex<HashMap<u64, Weak<RenderShared>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The UI side of libmpv's render API: one video surface of the application.
#[derive(Clone)]
pub struct MpvRenderHost {
    shared: Arc<RenderShared>,
}

impl std::fmt::Debug for MpvRenderHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MpvRenderHost")
            .field("id", &self.shared.id)
            .finish_non_exhaustive()
    }
}

impl Default for MpvRenderHost {
    fn default() -> Self {
        Self::new()
    }
}

impl MpvRenderHost {
    /// A new surface; pass [`MpvRenderHost::id`] in
    /// [`crate::MpvVideoMode::RenderApi`] to draw a player into it.
    #[must_use]
    pub fn new() -> Self {
        let shared = Arc::new(RenderShared {
            id: NEXT_HOST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            state: Mutex::new(RenderState::default()),
            freed: Condvar::new(),
        });
        if let Ok(mut hosts) = HOSTS.lock() {
            hosts.retain(|_, host| host.strong_count() > 0);
            hosts.insert(shared.id, Arc::downgrade(&shared));
        }
        Self { shared }
    }

    #[must_use]
    pub fn id(&self) -> u64 {
        self.shared.id
    }

    /// The function called (from an mpv thread) when a frame is ready; it
    /// should ask the drawing thread to redraw.
    ///
    /// # Safety
    ///
    /// `callback` must be safe to call from any thread with `context` for as
    /// long as this host exists.
    pub unsafe fn set_update_callback(&self, callback: UpdateCallback, context: *mut c_void) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.update_callback = Some((callback, context as usize));
            if state.context != 0
                && let Some(api) = state.api.as_ref()
            {
                // SAFETY: the context is live while the lock is held.
                unsafe {
                    (api.set_update_callback)(
                        state.context as *mut c_void,
                        Some(callback),
                        context,
                    );
                }
            }
        }
    }

    /// Draws the current video frame into the bound OpenGL framebuffer, or
    /// does the pending setup and teardown. Call it only on the drawing
    /// thread with its OpenGL context current. Returns `false` when there is
    /// nothing to draw.
    ///
    /// # Safety
    ///
    /// An OpenGL context must be current on the calling thread, `fbo` must be
    /// a framebuffer of that context, and the same thread must make every
    /// call of this host.
    pub unsafe fn draw(
        &self,
        get_proc_address: GetProcAddress,
        get_proc_address_context: *mut c_void,
        fbo: i32,
        width: i32,
        height: i32,
    ) -> bool {
        let Ok(mut state) = self.shared.state.lock() else {
            return false;
        };
        Self::free_if_detaching(&mut state, &self.shared.freed);
        let Some(api) = state.api.clone() else {
            return false;
        };
        if state.creation_failed {
            return false;
        }
        if state.context == 0 {
            let mut init = OpenGlInitParams {
                get_proc_address: Some(get_proc_address),
                get_proc_address_ctx: get_proc_address_context,
            };
            let mut params = [
                RenderParam {
                    kind: MPV_RENDER_PARAM_API_TYPE,
                    data: c"opengl".as_ptr().cast_mut().cast(),
                },
                RenderParam {
                    kind: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    data: (&raw mut init).cast(),
                },
                RenderParam {
                    kind: MPV_RENDER_PARAM_INVALID,
                    data: ptr::null_mut(),
                },
            ];
            let mut context = ptr::null_mut();
            let status = (api.create)(
                &raw mut context,
                state.client as *mut c_void,
                params.as_mut_ptr(),
            );
            if status < 0 || context.is_null() {
                log::error!("libmpv render context could not be created: {status}");
                state.creation_failed = true;
                self.shared.freed.notify_all();
                return false;
            }
            state.context = context as usize;
            self.shared.freed.notify_all();
            if let Some((callback, callback_context)) = state.update_callback {
                (api.set_update_callback)(context, Some(callback), callback_context as *mut c_void);
            }
        }
        let context = state.context as *mut c_void;
        let _ = (api.update)(context);
        let mut target = OpenGlFbo {
            fbo,
            w: width,
            h: height,
            internal_format: 0,
        };
        let mut flip_y: c_int = 1;
        let mut params = [
            RenderParam {
                kind: MPV_RENDER_PARAM_OPENGL_FBO,
                data: (&raw mut target).cast(),
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_FLIP_Y,
                data: (&raw mut flip_y).cast(),
            },
            RenderParam {
                kind: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        (api.render)(context, params.as_mut_ptr()) >= 0
    }

    /// Whether libmpv has a new frame to draw (`MPV_RENDER_UPDATE_FRAME`).
    #[must_use]
    pub fn wants_frame(&self) -> bool {
        let Ok(state) = self.shared.state.lock() else {
            return false;
        };
        if state.detaching {
            return true;
        }
        match (state.api.as_ref(), state.context) {
            (Some(_), 0) => true,
            // SAFETY: the context is live while the lock is held.
            (Some(api), context) => unsafe {
                (api.update)(context as *mut c_void) & MPV_RENDER_UPDATE_FRAME != 0
            },
            (None, _) => false,
        }
    }

    /// Tells libmpv that the drawn frame was shown.
    pub fn report_swap(&self) {
        if let Ok(state) = self.shared.state.lock()
            && state.context != 0
            && let Some(api) = state.api.as_ref()
        {
            // SAFETY: the context is live while the lock is held.
            unsafe { (api.report_swap)(state.context as *mut c_void) };
        }
    }

    /// Cancels setup before the closing UI joins the playback worker. This is
    /// terminal for this surface, including setup not yet attached; ordinary
    /// player Close/reopen keeps the host usable and does not call this method.
    /// An initialized video context is released by the normal teardown.
    pub fn cancel_pending_setup(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.setup_cancelled = true;
            if state.context == 0 {
                state.detaching = true;
                state.client = 0;
            }
            self.shared.freed.notify_all();
        }
    }

    /// Frees the render context when the surface goes away. Call it on the
    /// drawing thread with its OpenGL context current.
    ///
    /// # Safety
    ///
    /// As for [`MpvRenderHost::draw`].
    pub unsafe fn release(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            if state.context != 0
                && let Some(api) = state.api.as_ref()
            {
                (api.free)(state.context as *mut c_void);
            }
            state.context = 0;
            if state.detaching {
                state.api = None;
                state.client = 0;
                state.detaching = false;
            }
            self.shared.freed.notify_all();
        }
    }

    fn free_if_detaching(state: &mut RenderState, freed: &Condvar) {
        if !state.detaching {
            return;
        }
        if state.context != 0
            && let Some(api) = state.api.as_ref()
        {
            // SAFETY: called on the drawing thread by `draw`.
            unsafe { (api.free)(state.context as *mut c_void) };
        }
        state.context = 0;
        state.api = None;
        state.client = 0;
        state.detaching = false;
        freed.notify_all();
    }
}

/// The engine's link to a host while its client lives.
pub(crate) struct RenderAttachment {
    shared: Arc<RenderShared>,
}

impl RenderAttachment {
    /// Attaches an initialized client to the host registered as `id`.
    pub(crate) unsafe fn attach(
        id: u64,
        library: &Path,
        client: *mut c_void,
    ) -> Result<Self, PlaybackError> {
        let shared = HOSTS
            .lock()
            .ok()
            .and_then(|hosts| hosts.get(&id).and_then(Weak::upgrade))
            .ok_or_else(|| PlaybackError::Operation("the video surface is gone".to_owned()))?;
        let mut state = shared
            .state
            .lock()
            .map_err(|_| PlaybackError::Operation("video surface lock was poisoned".to_owned()))?;
        if state.setup_cancelled {
            return Err(PlaybackError::Operation(
                "video surface setup was cancelled".to_owned(),
            ));
        }
        let api = Arc::new(RenderApi::load(library)?);
        state.api = Some(api);
        state.client = client as usize;
        state.context = 0;
        state.detaching = false;
        state.creation_failed = false;
        let callback = state.update_callback;
        drop(state);
        // The surface draws once to create the render context.
        if let Some((callback, context)) = callback {
            callback(context as *mut c_void);
        }
        Ok(Self { shared })
    }

    /// Before loading video, the worker waits for AppKit to create the render
    /// context on its drawing thread. Once initialized this returns immediately.
    pub(crate) fn wait_until_ready(&self, timeout: Duration) -> bool {
        let Ok(state) = self.shared.state.lock() else {
            return false;
        };
        self.shared
            .freed
            .wait_timeout_while(state, timeout, |state| {
                state.context == 0
                    && state.client != 0
                    && !state.detaching
                    && !state.creation_failed
                    && !state.setup_cancelled
            })
            .is_ok_and(|(state, _)| {
                state.context != 0
                    && !state.detaching
                    && !state.creation_failed
                    && !state.setup_cancelled
            })
    }

    /// Asks the drawing thread to free the render context and waits for it.
    /// Returns `false` when it did not happen in time; the client must then
    /// not be destroyed.
    pub(crate) fn detach(&self, timeout: Duration) -> bool {
        let Ok(mut state) = self.shared.state.lock() else {
            return false;
        };
        if state.context == 0 {
            state.api = None;
            state.client = 0;
            self.shared.freed.notify_all();
            return true;
        }
        state.detaching = true;
        let callback = state.update_callback;
        drop(state);
        if let Some((callback, context)) = callback {
            // SAFETY: the UI's callback only asks for a redraw.
            unsafe { callback(context as *mut c_void) };
        }
        let Ok(state) = self.shared.state.lock() else {
            return false;
        };
        self.shared
            .freed
            .wait_timeout_while(state, timeout, |state| state.context != 0)
            .is_ok_and(|(state, _)| state.context == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending_attachment() -> RenderAttachment {
        RenderAttachment {
            shared: Arc::new(RenderShared {
                id: 0,
                state: Mutex::new(RenderState {
                    client: 1,
                    ..RenderState::default()
                }),
                freed: Condvar::new(),
            }),
        }
    }

    #[test]
    fn loading_waits_for_the_drawing_thread_to_initialize_the_context() {
        let attachment = pending_attachment();
        assert!(!attachment.wait_until_ready(Duration::ZERO));
        let shared = Arc::clone(&attachment.shared);
        let drawing = std::thread::spawn(move || {
            let mut state = shared.state.lock().unwrap();
            // An opaque readiness marker; no OpenGL functions are called here.
            state.context = 2;
            shared.freed.notify_all();
        });
        assert!(attachment.wait_until_ready(Duration::from_secs(1)));
        drawing.join().unwrap();
        assert!(attachment.wait_until_ready(Duration::ZERO));
    }

    #[test]
    fn failed_setup_or_shutdown_wakes_the_waiter_without_the_timeout() {
        for failure in [true, false] {
            let attachment = pending_attachment();
            let other = RenderAttachment {
                shared: Arc::clone(&attachment.shared),
            };
            let (done, receive) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                done.send(other.wait_until_ready(Duration::from_secs(5)))
                    .unwrap();
            });
            if failure {
                attachment.shared.state.lock().unwrap().creation_failed = true;
                attachment.shared.freed.notify_all();
            } else {
                let host = MpvRenderHost {
                    shared: Arc::clone(&attachment.shared),
                };
                host.cancel_pending_setup();
                assert!(attachment.detach(Duration::ZERO));
            }
            assert!(
                !receive
                    .recv_timeout(Duration::from_secs(1))
                    .expect("worker must not wait five seconds")
            );
            worker.join().unwrap();
        }
    }

    #[test]
    fn shutdown_before_attach_cannot_reset_the_cancellation() {
        let host = MpvRenderHost::new();
        host.cancel_pending_setup();
        // Cancellation is checked before loading any API or touching the client.
        let error = unsafe {
            RenderAttachment::attach(host.id(), Path::new("absent-library"), ptr::null_mut())
        };
        assert!(
            matches!(error, Err(PlaybackError::Operation(message)) if message.contains("cancelled"))
        );
        assert!(host.shared.state.lock().unwrap().setup_cancelled);
    }

    #[test]
    fn an_unavailable_or_detaching_surface_cannot_start_video() {
        let attachment = pending_attachment();
        assert!(!attachment.wait_until_ready(Duration::from_millis(1)));
        attachment.shared.state.lock().unwrap().client = 0;
        assert!(!attachment.wait_until_ready(Duration::ZERO));
        let mut state = attachment.shared.state.lock().unwrap();
        state.context = 2;
        state.detaching = true;
        drop(state);
        assert!(!attachment.wait_until_ready(Duration::ZERO));
    }
}
