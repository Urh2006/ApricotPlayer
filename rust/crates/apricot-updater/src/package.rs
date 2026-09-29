//! Download trust and package checks: Python `validate_trusted_download_url`,
//! `validate_https_response_url`, `verify_file_sha256`,
//! `verify_release_asset_file`, `validate_update_package` and
//! `validate_zip_archive`.

use std::{
    collections::HashSet,
    fmt::Write as _,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use sha2::{Digest, Sha256};
use url::Url;

use crate::release::{PackageNames, ReleaseAsset};

/// Python `UPDATE_ASSET_MAX_BYTES`.
pub const UPDATE_ASSET_MAX_BYTES: u64 = 1024 * 1024 * 1024;
/// Python `UPDATE_ZIP_MAX_*`.
pub const UPDATE_ZIP_MAX_ENTRIES: usize = 10_000;
pub const UPDATE_ZIP_MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const UPDATE_ZIP_MAX_MEMBER_BYTES: u64 = 512 * 1024 * 1024;
pub const UPDATE_ZIP_MAX_COMPRESSION_RATIO: u64 = 200;

/// Python `validate_trusted_download_url`: HTTPS and an exact host.
///
/// # Errors
/// `untrusted download URL: ...`.
pub fn validate_trusted_download_url(value: &str, allowed_hosts: &[&str]) -> Result<(), String> {
    let trusted = Url::parse(value).is_ok_and(|url| {
        url.scheme().eq_ignore_ascii_case("https")
            && url.host_str().is_some_and(|host| {
                allowed_hosts
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(host))
            })
    });
    if trusted {
        Ok(())
    } else {
        Err(format!("untrusted download URL: {value}"))
    }
}

/// Python `validate_https_response_url`: HTTPS and a host under one of the
/// roots.
///
/// # Errors
/// The Python messages for a non-HTTPS or untrusted final address.
pub fn validate_https_response_url(value: &str, allowed_roots: &[&str]) -> Result<(), String> {
    let url = Url::parse(value).ok();
    if !url
        .as_ref()
        .is_some_and(|url| url.scheme().eq_ignore_ascii_case("https"))
    {
        return Err(format!("download redirected to a non-HTTPS URL: {value}"));
    }
    if !allowed_roots.is_empty() && !host_under_roots(url.as_ref(), allowed_roots) {
        return Err(format!("download redirected to an untrusted host: {value}"));
    }
    Ok(())
}

/// Python `validate_trusted_https_url` for metadata responses.
///
/// # Errors
/// `{label} redirected to an untrusted address`.
pub fn validate_trusted_https_url(
    value: &str,
    allowed_roots: &[&str],
    label: &str,
) -> Result<(), String> {
    let url = Url::parse(value.trim()).ok();
    if url
        .as_ref()
        .is_some_and(|url| url.scheme().eq_ignore_ascii_case("https"))
        && host_under_roots(url.as_ref(), allowed_roots)
    {
        Ok(())
    } else {
        Err(format!("{label} redirected to an untrusted address"))
    }
}

fn host_under_roots(url: Option<&Url>, roots: &[&str]) -> bool {
    let Some(host) = url.and_then(Url::host_str) else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    !host.is_empty()
        && roots.iter().any(|root| {
            let root = root.to_ascii_lowercase();
            let root = root.trim_start_matches('.').trim_end_matches('.');
            host == root || host.ends_with(&format!(".{root}"))
        })
}

/// Python `file_sha256`.
///
/// # Errors
/// The file-system error text.
pub fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .fold(String::new(), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        }))
}

/// Python `verify_file_sha256`: an empty expectation accepts the file.
///
/// # Errors
/// A checksum mismatch or a read error.
pub fn verify_file_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let expected = expected.trim().to_lowercase();
    let expected = expected.strip_prefix("sha256:").unwrap_or(&expected);
    if expected.is_empty() {
        return Ok(());
    }
    if file_sha256(path)? == expected {
        Ok(())
    } else {
        Err("downloaded file checksum did not match the published SHA-256 digest".to_owned())
    }
}

/// `[0-9a-fA-F]{64}` with an optional `sha256:` prefix.
#[must_use]
pub fn is_sha256_digest(value: &str, allow_prefix: bool) -> bool {
    let value = value.trim();
    let value = if allow_prefix {
        value.strip_prefix("sha256:").unwrap_or(value)
    } else {
        value
    };
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Python `verify_release_asset_file`.
///
/// # Errors
/// A size or digest mismatch, or a missing digest.
pub fn verify_release_asset_file(asset: &ReleaseAsset, path: &Path) -> Result<(), String> {
    if let Some(size) = asset.size {
        let actual = std::fs::metadata(path)
            .map_err(|error| error.to_string())?
            .len();
        if actual != size {
            return Err(
                "downloaded update size did not match the GitHub release asset size".to_owned(),
            );
        }
    }
    if !is_sha256_digest(&asset.digest, true) {
        return Err(
            "GitHub did not publish a valid SHA-256 digest for this update asset".to_owned(),
        );
    }
    verify_file_sha256(path, &asset.digest)
}

/// Python `validate_update_package`, with the Rust package layout: the zip
/// holds one root folder with the executable, `components` and `mpv`.
///
/// # Errors
/// The Python error texts.
pub fn validate_update_package(package: &PackageNames, path: &Path) -> Result<(), String> {
    let size = std::fs::metadata(path).map(|metadata| metadata.len()).ok();
    if size.is_none_or(|size| size < 1024 * 1024) {
        return Err("downloaded update is not a valid package".to_owned());
    }
    let name = path.to_string_lossy();
    if package.is_portable_zip_asset(&name) {
        let entries = read_zip_entries(path)
            .map_err(|_| "downloaded portable update is not a valid zip file".to_owned())?;
        validate_zip_entries(&entries)?;
        return validate_portable_layout(package, &entries);
    }
    let mut header = [0_u8; 2];
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    if file.read_exact(&mut header).is_err() || &header != b"MZ" {
        return Err("downloaded update is not a Windows executable".to_owned());
    }
    Ok(())
}

/// What the zip central directory says about one member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZipEntry {
    pub name: String,
    pub flags: u16,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub external_attributes: u32,
}

/// Reads the zip central directory without extracting anything.
///
/// # Errors
/// A text describing the structural problem.
pub fn read_zip_entries(path: &Path) -> Result<Vec<ZipEntry>, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let length = file
        .seek(SeekFrom::End(0))
        .map_err(|error| error.to_string())?;
    let tail_length = length.min(65_557);
    file.seek(SeekFrom::Start(length - tail_length))
        .map_err(|error| error.to_string())?;
    let mut tail = vec![0_u8; usize::try_from(tail_length).map_err(|error| error.to_string())?];
    file.read_exact(&mut tail)
        .map_err(|error| error.to_string())?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|index| tail[*index..].starts_with(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or("missing end of central directory")?;
    let record = &tail[end..];
    let count = u16_at(record, 10);
    let directory_size = u32_at(record, 12);
    let directory_offset = u32_at(record, 16);
    if count == u16::MAX || directory_size == u32::MAX || directory_offset == u32::MAX {
        return Err("zip64 archives are not supported".to_owned());
    }
    if u64::from(directory_offset) + u64::from(directory_size) > length {
        return Err("central directory is out of range".to_owned());
    }
    file.seek(SeekFrom::Start(u64::from(directory_offset)))
        .map_err(|error| error.to_string())?;
    let mut directory = vec![0_u8; directory_size as usize];
    file.read_exact(&mut directory)
        .map_err(|error| error.to_string())?;
    let mut entries = Vec::with_capacity(usize::from(count));
    let mut offset = 0_usize;
    for _ in 0..count {
        let header = directory
            .get(offset..offset + 46)
            .ok_or("truncated entry")?;
        if !header.starts_with(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err("bad central directory entry".to_owned());
        }
        let name_length = usize::from(u16_at(header, 28));
        let extra_length = usize::from(u16_at(header, 30));
        let comment_length = usize::from(u16_at(header, 32));
        let name = directory
            .get(offset + 46..offset + 46 + name_length)
            .ok_or("truncated name")?;
        let flags = u16_at(header, 8);
        let name = if flags & 0x0800 == 0 {
            // Without the UTF-8 flag Python decodes names as code page 437;
            // ASCII names, the only ones a valid package has, match either way.
            name.iter().map(|byte| char::from(*byte)).collect()
        } else {
            String::from_utf8_lossy(name).into_owned()
        };
        entries.push(ZipEntry {
            name,
            flags,
            compressed_size: u64::from(u32_at(header, 20)),
            uncompressed_size: u64::from(u32_at(header, 24)),
            external_attributes: u32_at(header, 38),
        });
        offset += 46 + name_length + extra_length + comment_length;
    }
    Ok(entries)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Python `validate_zip_member_path`.
fn validate_zip_member_path(name: &str) -> Result<(), String> {
    let normalized = name.replace('\\', "/");
    let unsafe_path = normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.contains('\0')
        || normalized.split('/').any(|part| part == "..");
    if unsafe_path {
        Err("zip package contains an unsafe path".to_owned())
    } else {
        Ok(())
    }
}

/// Python `validate_zip_archive`.
///
/// # Errors
/// The Python error texts.
pub fn validate_zip_entries(entries: &[ZipEntry]) -> Result<(), String> {
    if entries.len() > UPDATE_ZIP_MAX_ENTRIES {
        return Err("zip package contains too many entries".to_owned());
    }
    let mut total = 0_u64;
    let mut names = HashSet::new();
    for entry in entries {
        validate_zip_member_path(&entry.name)?;
        let normalized = entry
            .name
            .replace('\\', "/")
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/")
            .to_lowercase();
        if !names.insert(normalized) {
            return Err("zip package contains duplicate paths".to_owned());
        }
        if entry.flags & 0x1 != 0 {
            return Err("zip package contains an encrypted entry".to_owned());
        }
        let file_type = (entry.external_attributes >> 16) & 0o170_000;
        if ![0, 0o100_000, 0o040_000].contains(&file_type) {
            return Err("zip package contains a link or special file".to_owned());
        }
        if entry.uncompressed_size > UPDATE_ZIP_MAX_MEMBER_BYTES {
            return Err("zip package member is too large".to_owned());
        }
        total += entry.uncompressed_size;
        if total > UPDATE_ZIP_MAX_UNCOMPRESSED_BYTES {
            return Err("zip package expands beyond the safe size limit".to_owned());
        }
        if entry.uncompressed_size > 1024 * 1024
            && (entry.compressed_size == 0
                || entry.uncompressed_size / entry.compressed_size
                    > UPDATE_ZIP_MAX_COMPRESSION_RATIO
                || (entry.uncompressed_size / entry.compressed_size
                    == UPDATE_ZIP_MAX_COMPRESSION_RATIO
                    && entry.uncompressed_size % entry.compressed_size != 0))
        {
            return Err("zip package has an unsafe compression ratio".to_owned());
        }
    }
    Ok(())
}

/// Top-level items a Rust package may carry next to the executable.
const PORTABLE_ITEMS: [&str; 7] = [
    "assets",
    "components",
    "ffmpeg",
    "mpv",
    "nvda",
    "build-info.json",
    "package-manifest.json",
];

fn validate_portable_layout(package: &PackageNames, entries: &[ZipEntry]) -> Result<(), String> {
    let root = package.portable_root.to_lowercase();
    let executable = format!("{root}/{}", package.executable.to_lowercase());
    let names: Vec<String> = entries
        .iter()
        .map(|entry| {
            entry
                .name
                .replace('\\', "/")
                .trim_matches('/')
                .to_lowercase()
        })
        .collect();
    let allowed = |name: &str| {
        if name == root || name == executable {
            return true;
        }
        name.strip_prefix(&format!("{root}/")).is_some_and(|rest| {
            let first = rest.split('/').next().unwrap_or_default();
            PORTABLE_ITEMS.contains(&first)
        })
    };
    if names.iter().any(|name| !allowed(name)) {
        return Err("downloaded portable update contains an unexpected file layout".to_owned());
    }
    if !names.contains(&executable) {
        return Err(format!(
            "downloaded portable update does not contain {}/{}",
            package.portable_root, package.executable
        ));
    }
    for required in ["components", "mpv"] {
        let folder = format!("{root}/{required}");
        if !names
            .iter()
            .any(|name| *name == folder || name.starts_with(&format!("{folder}/")))
        {
            return Err(format!(
                "downloaded portable update does not contain {}/{required}",
                package.portable_root
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use super::{
        ZipEntry, file_sha256, read_zip_entries, validate_https_response_url,
        validate_trusted_download_url, validate_trusted_https_url, validate_update_package,
        validate_zip_entries, verify_release_asset_file,
    };
    use crate::release::{RUST_BETA_PACKAGE, ReleaseAsset};

    /// A stored (uncompressed) zip with the given members.
    pub(crate) fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut output = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in members {
            let offset = u32::try_from(output.len()).expect("offset");
            let size = u32::try_from(data.len()).expect("size");
            let name_length = u16::try_from(name.len()).expect("name");
            output.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            output.extend_from_slice(&0_u32.to_le_bytes());
            output.extend_from_slice(&size.to_le_bytes());
            output.extend_from_slice(&size.to_le_bytes());
            output.extend_from_slice(&name_length.to_le_bytes());
            output.extend_from_slice(&0_u16.to_le_bytes());
            output.extend_from_slice(name.as_bytes());
            output.extend_from_slice(data);
            directory.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0]);
            directory.extend_from_slice(&[0, 0, 0, 0]);
            directory.extend_from_slice(&0_u32.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&size.to_le_bytes());
            directory.extend_from_slice(&name_length.to_le_bytes());
            directory.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
            directory.extend_from_slice(&0_u32.to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = u32::try_from(output.len()).expect("offset");
        let count = u16::try_from(members.len()).expect("count");
        output.extend_from_slice(&directory);
        output.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        output.extend_from_slice(&count.to_le_bytes());
        output.extend_from_slice(&count.to_le_bytes());
        output.extend_from_slice(&u32::try_from(directory.len()).expect("size").to_le_bytes());
        output.extend_from_slice(&directory_offset.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        output
    }

    #[test]
    fn urls_are_checked_like_python() {
        assert!(validate_trusted_download_url("https://github.com/a/b", &["github.com"]).is_ok());
        assert_eq!(
            validate_trusted_download_url("http://github.com/a", &["github.com"]),
            Err("untrusted download URL: http://github.com/a".to_owned())
        );
        assert!(
            validate_trusted_download_url("https://evil.github.com/a", &["github.com"]).is_err()
        );
        assert!(
            validate_https_response_url(
                "https://release-assets.githubusercontent.com/x",
                &["github.com", "githubusercontent.com"]
            )
            .is_ok()
        );
        assert_eq!(
            validate_https_response_url("https://evilgithub.com/x", &["github.com"]),
            Err("download redirected to an untrusted host: https://evilgithub.com/x".to_owned())
        );
        assert!(validate_https_response_url("http://github.com/x", &[]).is_err());
        assert_eq!(
            validate_trusted_https_url(
                "https://example.com",
                &["github.com"],
                "GitHub release metadata"
            ),
            Err("GitHub release metadata redirected to an untrusted address".to_owned())
        );
    }

    #[test]
    fn release_assets_need_the_published_size_and_digest() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("ApricotPlayer2Beta.zip");
        fs::write(&path, b"abc").expect("write");
        let digest = file_sha256(&path).expect("digest");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut asset = ReleaseAsset {
            size: Some(3),
            digest: format!("sha256:{digest}"),
            ..ReleaseAsset::default()
        };
        assert_eq!(verify_release_asset_file(&asset, &path), Ok(()));
        asset.size = Some(4);
        assert!(verify_release_asset_file(&asset, &path).is_err());
        asset.size = None;
        asset.digest = String::new();
        assert_eq!(
            verify_release_asset_file(&asset, &path),
            Err("GitHub did not publish a valid SHA-256 digest for this update asset".to_owned())
        );
        asset.digest = "0".repeat(64);
        assert_eq!(
            verify_release_asset_file(&asset, &path),
            Err("downloaded file checksum did not match the published SHA-256 digest".to_owned())
        );
    }

    #[test]
    fn portable_packages_need_the_rust_layout() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("ApricotPlayer2Beta.zip");
        let big = vec![b'M'; 1024 * 1024];
        let good = zip_bytes(&[
            ("ApricotPlayer2Beta/", b""),
            ("ApricotPlayer2Beta/ApricotPlayer2Beta.exe", &big),
            ("ApricotPlayer2Beta/components/yt-dlp.exe", b"x"),
            ("ApricotPlayer2Beta/mpv/libmpv-2.dll", b"x"),
        ]);
        fs::write(&path, &good).expect("zip");
        assert_eq!(read_zip_entries(&path).expect("entries").len(), 4);
        assert_eq!(validate_update_package(&RUST_BETA_PACKAGE, &path), Ok(()));
        let odd = zip_bytes(&[
            ("ApricotPlayer2Beta/ApricotPlayer2Beta.exe", &big),
            ("ApricotPlayer2Beta/components/a", b"x"),
            ("ApricotPlayer2Beta/mpv/a", b"x"),
            ("ApricotPlayer2Beta/evil.dll", b"x"),
        ]);
        fs::write(&path, &odd).expect("zip");
        assert_eq!(
            validate_update_package(&RUST_BETA_PACKAGE, &path),
            Err("downloaded portable update contains an unexpected file layout".to_owned())
        );
        let missing = zip_bytes(&[
            ("ApricotPlayer2Beta/ApricotPlayer2Beta.exe", &big),
            ("ApricotPlayer2Beta/components/a", b"x"),
        ]);
        fs::write(&path, &missing).expect("zip");
        assert_eq!(
            validate_update_package(&RUST_BETA_PACKAGE, &path),
            Err("downloaded portable update does not contain ApricotPlayer2Beta/mpv".to_owned())
        );
        let mut garbage = vec![0_u8; 1024 * 1024 + 10];
        garbage[0] = b'P';
        fs::write(&path, &garbage).expect("garbage");
        assert_eq!(
            validate_update_package(&RUST_BETA_PACKAGE, &path),
            Err("downloaded portable update is not a valid zip file".to_owned())
        );
    }

    #[test]
    fn installers_must_be_executables_of_at_least_one_mebibyte() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("ApricotPlayer2BetaSetup.exe");
        fs::write(&path, b"MZ").expect("small");
        assert_eq!(
            validate_update_package(&RUST_BETA_PACKAGE, &path),
            Err("downloaded update is not a valid package".to_owned())
        );
        let mut file = fs::File::create(&path).expect("file");
        file.write_all(&vec![b'Z'; 1024 * 1024]).expect("write");
        drop(file);
        assert_eq!(
            validate_update_package(&RUST_BETA_PACKAGE, &path),
            Err("downloaded update is not a Windows executable".to_owned())
        );
    }

    #[test]
    fn zip_members_are_bounded() {
        let entry = |name: &str| ZipEntry {
            name: name.to_owned(),
            flags: 0,
            compressed_size: 1,
            uncompressed_size: 1,
            external_attributes: 0,
        };
        assert!(validate_zip_entries(&[entry("a/b")]).is_ok());
        for bad in ["../a", "/a", "C:/a", "a\\..\\b"] {
            assert_eq!(
                validate_zip_entries(&[entry(bad)]),
                Err("zip package contains an unsafe path".to_owned())
            );
        }
        assert_eq!(
            validate_zip_entries(&[entry("A/b"), entry("a//B")]),
            Err("zip package contains duplicate paths".to_owned())
        );
        let mut encrypted = entry("a");
        encrypted.flags = 1;
        assert!(validate_zip_entries(&[encrypted]).is_err());
        let mut link = entry("a");
        link.external_attributes = 0o120_777 << 16;
        assert_eq!(
            validate_zip_entries(&[link]),
            Err("zip package contains a link or special file".to_owned())
        );
        let mut bomb = entry("a");
        bomb.uncompressed_size = 300 * 1024 * 1024;
        bomb.compressed_size = 1024 * 1024;
        assert_eq!(
            validate_zip_entries(&[bomb]),
            Err("zip package has an unsafe compression ratio".to_owned())
        );
    }
}
