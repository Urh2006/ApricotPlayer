//! The remembered `AudioVault` password, protected with Windows DPAPI exactly
//! as Python `protect_audiovault_password` stores it, so the Python version's
//! saved password keeps working.
//!
//! On macOS the secret itself is a Keychain item and the stored text is only
//! a reference to it ([`KEYCHAIN_PREFIX`]). A DPAPI blob copied from Windows
//! does not decrypt there, exactly like a blob of another Windows user.

#![cfg_attr(windows, allow(unsafe_code))]

const ENTROPY: &[u8] = b"ApricotPlayer AudioVault credentials v1";
#[cfg(windows)]
const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

/// Python `protect_audiovault_password`: base64 of the DPAPI blob, or an
/// empty text without a password.
///
/// # Errors
///
/// Returns the Windows error when DPAPI refuses the data.
pub fn protect_password(password: &str) -> Result<String, String> {
    if password.is_empty() {
        return Ok(String::new());
    }
    protect_data(password.as_bytes(), ENTROPY, "ApricotPlayer AudioVault")
}

/// Protects any data for the current Windows user with DPAPI and an
/// application-specific entropy; returns base64 of the blob.
///
/// # Errors
///
/// Returns the Windows error when DPAPI refuses the data.
#[cfg(not(target_os = "macos"))]
pub fn protect_data(data: &[u8], entropy: &[u8], description: &str) -> Result<String, String> {
    protect(data, entropy, description).map(|blob| crate::browser_cookies::base64_encode(&blob))
}

/// Reverses [`protect_data`]; `None` for anything that does not decrypt.
pub fn unprotect_data(value: &str, entropy: &[u8]) -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    if let Some(account) = value.strip_prefix(KEYCHAIN_PREFIX) {
        return keychain::read(account, entropy);
    }
    unprotect(&base64_decode(value)?, entropy)
}

/// Python `unprotect_audiovault_password`: an empty text for anything that
/// does not decrypt.
pub fn unprotect_password(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    unprotect_data(value, ENTROPY)
        .and_then(|data| String::from_utf8(data).ok())
        .unwrap_or_default()
}

/// Python `base64.b64decode(value, validate=True)`.
fn base64_decode(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let digit = |byte: u8| -> Option<u32> {
        Some(u32::from(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let mut output = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut value = 0_u32;
        for byte in &chunk[..4 - padding] {
            value = (value << 6) | digit(*byte)?;
        }
        value <<= 6 * u32::try_from(padding).unwrap_or_default();
        let produced = 3 - padding;
        for shift in [16, 8, 0].into_iter().take(produced) {
            output.push(u8::try_from((value >> shift) & 0xff).unwrap_or_default());
        }
    }
    Some(output)
}

#[cfg(windows)]
fn protect(data: &[u8], entropy: &[u8], description: &str) -> Result<Vec<u8>, String> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData},
        },
        core::PCWSTR,
    };
    let description: Vec<u16> = description.encode_utf16().chain(Some(0)).collect();
    let input = blob(data);
    let entropy = blob(entropy);
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: The input blobs point at live slices that DPAPI only reads, and
    // the output blob is freed with LocalFree after it has been copied.
    unsafe {
        CryptProtectData(
            &raw const input,
            PCWSTR(description.as_ptr()),
            Some(&raw const entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut output,
        )
        .map_err(|error| error.to_string())?;
        let result = copy_blob(&output);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(result)
    }
}

#[cfg(windows)]
fn unprotect(data: &[u8], entropy: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData},
    };
    let input = blob(data);
    let entropy = blob(entropy);
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: As in `protect`.
    unsafe {
        CryptUnprotectData(
            &raw const input,
            None,
            Some(&raw const entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut output,
        )
        .ok()?;
        let result = copy_blob(&output);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Some(result)
    }
}

#[cfg(windows)]
fn blob(data: &[u8]) -> windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
    windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).unwrap_or(u32::MAX),
        pbData: data.as_ptr().cast_mut(),
    }
}

#[cfg(windows)]
unsafe fn copy_blob(blob: &windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if blob.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: DPAPI returned `cbData` readable bytes at `pbData`.
    unsafe { std::slice::from_raw_parts(blob.pbData, usize::try_from(blob.cbData).unwrap_or(0)) }
        .to_vec()
}

/// The start of a stored value that names a Keychain item.
#[cfg(target_os = "macos")]
pub const KEYCHAIN_PREFIX: &str = "keychain-v1:";

/// macOS: the protected value is a reference to a Keychain item; see
/// [`keychain`].
///
/// # Errors
///
/// Returns the Keychain error when the item cannot be saved.
#[cfg(target_os = "macos")]
pub fn protect_data(data: &[u8], entropy: &[u8], description: &str) -> Result<String, String> {
    keychain::store(data, entropy, description).map(|account| format!("{KEYCHAIN_PREFIX}{account}"))
}

/// Removes the Keychain item a protected value refers to, after the
/// application has forgotten that value. Values that are not Keychain
/// references are ignored.
pub fn forget_protected(value: &str) {
    #[cfg(target_os = "macos")]
    if let Some(account) = value.strip_prefix(KEYCHAIN_PREFIX) {
        keychain::delete(account);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = value;
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    /// One Keychain service per application identity, so the beta never
    /// reads or replaces the stable version's secrets.
    fn service() -> &'static str {
        crate::BUILD_IDENTITY.folder_names().0
    }

    /// The item name: the description and a digest of the purpose (entropy)
    /// and the data, so saving the same secret again reuses its item and
    /// different accounts never share one.
    fn account_name(data: &[u8], entropy: &[u8], description: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(entropy);
        digest.update([0]);
        digest.update(data);
        let digest = digest.finalize();
        let mut name = description.replace(':', " ");
        name.push(' ');
        for byte in &digest[..12] {
            let _ = write!(name, "{byte:02x}");
        }
        name
    }

    /// The digest in the item name binds it to `entropy`, as DPAPI's entropy
    /// does; a reference used with another purpose does not resolve.
    fn entropy_matches(account: &str, data: &[u8], entropy: &[u8]) -> bool {
        let description = account.rsplit_once(' ').map_or("", |(prefix, _)| prefix);
        account_name(data, entropy, description) == account
    }

    pub(super) fn store(data: &[u8], entropy: &[u8], description: &str) -> Result<String, String> {
        let account = account_name(data, entropy, description);
        set_generic_password(service(), &account, data)
            .map(|()| account)
            .map_err(|error| error.to_string())
    }

    pub(super) fn read(account: &str, entropy: &[u8]) -> Option<Vec<u8>> {
        get_generic_password(service(), account)
            .ok()
            .filter(|data| entropy_matches(account, data, entropy))
    }

    pub(super) fn delete(account: &str) {
        let _ = delete_generic_password(service(), account);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
#[allow(clippy::unnecessary_wraps)] // Same signature as the Windows implementation.
fn protect(_data: &[u8], _entropy: &[u8], _description: &str) -> Result<Vec<u8>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
fn unprotect(_data: &[u8], _entropy: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::protect_password;
    use super::{base64_decode, unprotect_password};

    #[test]
    fn decodes_strict_base64_like_python() {
        assert_eq!(base64_decode("Zm9vYmFy"), Some(b"foobar".to_vec()));
        assert_eq!(base64_decode("Zg=="), Some(b"f".to_vec()));
        assert_eq!(base64_decode("Zm8="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("Zm9v!mFy"), None);
        assert_eq!(base64_decode("Zg="), None);
        assert_eq!(base64_decode("Zg==Zg=="), None);
    }

    /// Writes, reads and removes a real item in the login Keychain.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "uses the login Keychain, which may ask for permission"]
    fn keychain_references_round_trip_and_are_bound_to_their_purpose() {
        let protected = super::protect_data(b"secret", b"test entropy", "ApricotPlayer Test")
            .expect("Keychain item");
        assert!(protected.starts_with(super::KEYCHAIN_PREFIX));
        assert_eq!(
            super::unprotect_data(&protected, b"test entropy").as_deref(),
            Some(&b"secret"[..])
        );
        assert_eq!(super::unprotect_data(&protected, b"other purpose"), None);
        super::forget_protected(&protected);
        assert_eq!(super::unprotect_data(&protected, b"test entropy"), None);
    }

    #[cfg(windows)]
    #[test]
    fn protected_passwords_round_trip_for_this_user() {
        let protected = protect_password("sečret pass").expect("protect");
        assert!(!protected.is_empty());
        assert_ne!(protected, "sečret pass");
        assert_eq!(unprotect_password(&protected), "sečret pass");
        assert_eq!(unprotect_password("not base64!"), "");
        assert_eq!(unprotect_password("Zm9vYmFy"), "");
        assert_eq!(protect_password("").expect("empty"), "");
    }

    /// A password saved by Python `protect_audiovault_password` for this
    /// Windows user, passed in `APRICOT_TEST_PYTHON_AUDIOVAULT_BLOB` with the
    /// clear text in `APRICOT_TEST_PYTHON_AUDIOVAULT_PASSWORD`.
    #[test]
    #[ignore = "needs a blob made by the Python version for this user"]
    fn reads_the_python_version_password() {
        let blob = std::env::var("APRICOT_TEST_PYTHON_AUDIOVAULT_BLOB").expect("blob");
        let password = std::env::var("APRICOT_TEST_PYTHON_AUDIOVAULT_PASSWORD").expect("password");
        assert_eq!(unprotect_password(&blob), password);
    }
}
