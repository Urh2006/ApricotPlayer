//! The "default media player" setting on macOS (Windows
//! `set_default_player`): Launch Services makes this bundle the viewer of
//! the common audio and video types. The types are declared in the
//! bundle's `Info.plist` (unit M15).

#![allow(unsafe_code)]

use std::ffi::c_void;

use objc2_foundation::{NSBundle, NSString};

/// The audio and video types the player opens.
const CONTENT_TYPES: &[&str] = &[
    "public.mp3",
    "public.mpeg-4-audio",
    "com.apple.m4a-audio",
    "public.aac-audio",
    "public.aiff-audio",
    "com.microsoft.waveform-audio",
    "org.xiph.flac",
    "org.xiph.ogg-audio",
    "org.xiph.opus",
    "public.mpeg-4",
    "com.apple.quicktime-movie",
    "public.avi",
    "public.mpeg",
    "org.matroska.mkv",
    "org.webmproject.webm",
];

/// `kLSRolesViewer`.
const ROLE_VIEWER: u32 = 0x0000_0002;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSSetDefaultRoleHandlerForContentType(
        content_type: *const c_void,
        role: u32,
        handler_bundle_id: *const c_void,
    ) -> i32;
}

/// Makes this application the default player.
///
/// # Errors
///
/// Returns the reason when it does not run from its bundle or Launch
/// Services refused every type.
pub fn set() -> Result<(), String> {
    let Some(bundle) = NSBundle::mainBundle().bundleIdentifier() else {
        return Err("ApricotPlayer must run from its application bundle".to_owned());
    };
    let mut last_error = 0;
    let mut set_any = false;
    for content_type in CONTENT_TYPES {
        let content_type = NSString::from_str(content_type);
        // SAFETY: NSString is toll-free bridged to CFStringRef; both strings
        // live until the call returns.
        let status = unsafe {
            LSSetDefaultRoleHandlerForContentType(
                std::ptr::from_ref(&*content_type).cast(),
                ROLE_VIEWER,
                std::ptr::from_ref(&*bundle).cast(),
            )
        };
        if status == 0 {
            set_any = true;
        } else {
            last_error = status;
        }
    }
    if set_any {
        Ok(())
    } else {
        Err(format!("Launch Services error {last_error}"))
    }
}
