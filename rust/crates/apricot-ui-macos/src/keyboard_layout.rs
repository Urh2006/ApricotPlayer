//! Resolve character hotkeys through the currently selected macOS layout.
#![allow(unsafe_code)]
use objc2_foundation::NSString;
use std::ffi::c_void;

type Ref = *const c_void;
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> Ref;
    fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> Ref;
    fn TISGetInputSourceProperty(source: Ref, key: Ref) -> Ref;
    static kTISPropertyUnicodeKeyLayoutData: Ref;
    static kTISNotifySelectedKeyboardInputSourceChanged: Ref;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: Ref,
        code: u16,
        action: u16,
        modifiers: u32,
        keyboard: u32,
        options: u32,
        dead_state: *mut u32,
        capacity: u32,
        length: *mut u32,
        output: *mut u16,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFRelease(object: Ref);
}
struct Layout {
    source: Ref,
    bytes: Ref,
}
impl Drop for Layout {
    fn drop(&mut self) {
        // SAFETY: the input source was returned by a Copy API and is released once.
        unsafe {
            CFRelease(self.source);
        }
    }
}
impl Layout {
    fn current() -> Option<Self> {
        // SAFETY: TIS returns retained sources and borrowed CFData properties.
        // The retained source keeps its layout bytes alive through translation.
        unsafe {
            for fallback in [false, true] {
                let source = if fallback {
                    TISCopyCurrentASCIICapableKeyboardLayoutInputSource()
                } else {
                    TISCopyCurrentKeyboardLayoutInputSource()
                };
                if source.is_null() {
                    continue;
                }
                let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
                if !data.is_null() && CFDataGetLength(data) > 0 {
                    let bytes: Ref = CFDataGetBytePtr(data).cast();
                    if !bytes.is_null() {
                        return Some(Self { source, bytes });
                    }
                }
                CFRelease(source);
            }
        }
        None
    }
    fn character(&self, code: u16) -> Option<char> {
        let mut state = 0;
        let mut length = 0;
        let mut output = [0; 4];
        // SAFETY: the trusted layout and all typed output buffers remain live;
        // kUCKeyActionDown=0, no modifier state, kUCKeyTranslateNoDeadKeysMask=1.
        let status = unsafe {
            UCKeyTranslate(
                self.bytes,
                code,
                0,
                0,
                u32::from(LMGetKbdType()),
                1,
                &raw mut state,
                4,
                &raw mut length,
                output.as_mut_ptr(),
            )
        };
        if status != 0 || length != 1 {
            return None;
        }
        char::from_u32(u32::from(output[0]))
    }
}
pub fn virtual_key(character: char) -> Option<u32> {
    let layout = Layout::current()?;
    find_key(character, |code| layout.character(code))
}
fn find_key(character: char, mut translate: impl FnMut(u16) -> Option<char>) -> Option<u32> {
    (0_u16..128)
        .find(|code| {
            translate(*code)
                .is_some_and(|candidate| candidate.to_lowercase().eq(character.to_lowercase()))
        })
        .map(u32::from)
}
pub fn notification_name() -> &'static NSString {
    // SAFETY: Apple's notification constant is an immutable process-lifetime
    // CFString, toll-free bridged to NSString.
    unsafe { &*kTISNotifySelectedKeyboardInputSourceChanged.cast::<NSString>() }
}
#[cfg(test)]
mod tests {
    use super::find_key;
    #[test]
    fn unicode_layout_case_matching_does_not_assume_ascii() {
        let layout = |code| if code == 3 { Some('Č') } else { None };
        assert_eq!(find_key('č', layout), Some(3));
    }
    #[test]
    fn non_qwerty_layout_changes_character_positions() {
        let azerty = |code| match code {
            0 => Some('q'),
            12 => Some('a'),
            6 => Some('y'),
            16 => Some('z'),
            _ => None,
        };
        assert_eq!(find_key('Q', azerty), Some(0));
        assert_eq!(find_key('a', azerty), Some(12));
        assert_eq!(find_key('Y', azerty), Some(6));
        assert_eq!(find_key('k', azerty), None);
    }
}
