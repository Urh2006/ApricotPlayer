//! VoiceOver announcements, the macOS counterpart of the NVDA controller
//! path in `announcement_win32.rs`.

#![allow(unsafe_code)]

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
    NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey,
    NSAccessibilityPriorityLevel, NSApplication,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

/// Speaks `text` once without moving focus. `interrupt` cuts off current
/// speech (NVDA `cancelSpeech` before `speakText`); otherwise the text
/// follows what VoiceOver is saying.
pub fn announce(mtm: MainThreadMarker, text: &str, interrupt: bool) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let app = NSApplication::sharedApplication(mtm);
    let message = NSString::from_str(text);
    let priority = if interrupt {
        NSAccessibilityPriorityLevel::High
    } else {
        NSAccessibilityPriorityLevel::Medium
    };
    let priority = NSNumber::new_isize(priority.0);
    // SAFETY: both keys are AppKit's static user-info keys and the values have
    // the documented types (`NSString` and `NSNumber`).
    unsafe {
        let info: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
            &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
            &[
                message.as_ref() as &AnyObject,
                priority.as_ref() as &AnyObject,
            ],
        );
        NSAccessibilityPostNotificationWithUserInfo(
            &app,
            NSAccessibilityAnnouncementRequestedNotification,
            Some(&info),
        );
    }
}
