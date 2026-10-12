//! System notifications (Windows `show_tray_notification`) through the
//! User Notifications framework. macOS asks once for permission; the
//! notifications also show while the application is in front, as the Windows
//! balloons do. They need the application bundle, so a development build
//! started from the terminal shows none.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotNotificationDelegate"]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        /// A notification while ApricotPlayer is active still shows.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }
    }
);

thread_local! {
    static DELEGATE: RefCell<Option<Retained<NotificationDelegate>>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
}

/// The notification center, only inside the application bundle (it throws
/// an Objective-C exception elsewhere).
fn center(mtm: MainThreadMarker) -> Option<Retained<UNUserNotificationCenter>> {
    NSBundle::mainBundle().bundleIdentifier()?;
    let center = UNUserNotificationCenter::currentNotificationCenter();
    DELEGATE.with(|delegate| {
        let mut delegate = delegate.borrow_mut();
        if delegate.is_none() {
            let this = NotificationDelegate::alloc(mtm).set_ivars(());
            // SAFETY: `init` of NSObject.
            let this: Retained<NotificationDelegate> = unsafe { msg_send![super(this), init] };
            center.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            *delegate = Some(this);
        }
    });
    Some(center)
}

/// Shows `title` and `message`, asking for permission the first time.
pub fn notify(mtm: MainThreadMarker, title: &str, message: &str) {
    let Some(center) = center(mtm) else {
        return;
    };
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(message));
    let id = NEXT_ID.with(|next| {
        next.set(next.get() + 1);
        next.get()
    });
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&format!("apricot-{}-{id}", std::process::id())),
        &content,
        None,
    );
    let post = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if granted.as_bool() {
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, None);
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &post,
    );
}
