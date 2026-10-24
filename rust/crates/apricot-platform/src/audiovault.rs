//! `AudioVault` network access, TV show packages and the playback cache, as
//! Python `apricot/network/audiovault.py` does them.

use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use reqwest::{
    blocking::{Client, Response},
    cookie::{CookieStore, Jar},
    header::{ACCEPT, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE},
};
use url::Url;

use crate::zip_archive::{self, ZipEntry};

pub const AUDIOVAULT_BASE_URL: &str = "https://direct.audiovault.net";
pub const AUDIOVAULT_REGISTER_URL: &str = "https://direct.audiovault.net/register";
const MAX_RESPONSE: u64 = 8 * 1024 * 1024;
pub const MAX_ARCHIVE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 16 * 1024 * 1024 * 1024;
const MAX_RANGE_READ: u64 = 8 * 1024 * 1024;
const RANGE_CACHE: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const CHUNK: usize = 1024 * 1024;
pub const AUDIO_EXTENSIONS: [&str; 9] = [
    ".aac", ".flac", ".m4a", ".m4b", ".mp3", ".ogg", ".opus", ".wav", ".wma",
];
const TRUSTED_HOSTS: [&str; 3] = [
    "direct.audiovault.net",
    "www.audiovault.net",
    "audiovault.net",
];

/// Python raises these as `ValueError(self.t(key))` or its own exception
/// classes; `Message` keeps Python's English texts and transport errors.
#[derive(Debug, thiserror::Error)]
pub enum AudiovaultError {
    #[error("audiovault_untrusted_url")]
    UntrustedUrl,
    #[error("audiovault_login_page_error")]
    LoginPage,
    #[error("audiovault_login_failed")]
    LoginFailed,
    /// Python `AudioVaultSessionExpired`.
    #[error("audiovault_session_expired")]
    SessionExpired,
    /// Python `AudioVaultRangeUnsupported`.
    #[error("AudioVault does not support partial archive reads.")]
    RangeUnsupported,
    #[error("audiovault_archive_too_large")]
    ArchiveTooLarge,
    #[error("audiovault_show_format_error")]
    ShowFormat,
    #[error("audiovault_no_episodes")]
    NoEpisodes,
    #[error("{0}")]
    Message(String),
}

impl AudiovaultError {
    /// The locale key of an error Python shows with `self.t`.
    pub const fn text_key(&self) -> Option<&'static str> {
        match self {
            Self::UntrustedUrl => Some("audiovault_untrusted_url"),
            Self::LoginPage => Some("audiovault_login_page_error"),
            Self::LoginFailed => Some("audiovault_login_failed"),
            Self::SessionExpired => Some("audiovault_session_expired"),
            Self::ArchiveTooLarge => Some("audiovault_archive_too_large"),
            Self::ShowFormat => Some("audiovault_show_format_error"),
            Self::NoEpisodes => Some("audiovault_no_episodes"),
            Self::RangeUnsupported | Self::Message(_) => None,
        }
    }

    fn message(value: impl Into<String>) -> Self {
        Self::Message(value.into())
    }
}

impl From<io::Error> for AudiovaultError {
    fn from(error: io::Error) -> Self {
        Self::Message(error.to_string())
    }
}

impl From<reqwest::Error> for AudiovaultError {
    fn from(error: reqwest::Error) -> Self {
        Self::Message(error.to_string())
    }
}

/// Python's `urllib` opener with its cookie jar. Logging out replaces the
/// client and its jar, as `audiovault_cookie_jar.clear()` does.
pub struct AudiovaultClient {
    user_agent: String,
    session: Mutex<(Client, Arc<Jar>)>,
    /// A local stand-in for `AudioVault` in local beta builds.
    test_base: Option<String>,
}

/// The local test server address of a local beta, from this variable.
pub const AUDIOVAULT_TEST_BASE_VARIABLE: &str = "APRICOT_AUDIOVAULT_TEST_BASE";

/// A loopback address given in `APRICOT_AUDIOVAULT_TEST_BASE`, honored only in
/// builds that cannot install updates, like the update test feed.
fn test_base_from_environment() -> Option<String> {
    if apricot_updater::BUILD_CHANNEL.allows_remote_install() {
        return None;
    }
    let value = std::env::var(AUDIOVAULT_TEST_BASE_VARIABLE).ok()?;
    let url = Url::parse(value.trim()).ok()?;
    (url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost")))
        .then(|| value.trim().trim_end_matches('/').to_owned())
}

/// Python `resolve_audiovault_stream`: the open response, its final address
/// and the headers mpv needs to play it.
pub struct AudiovaultStream {
    pub response: Response,
    pub final_url: String,
    pub content_type: String,
    pub disposition: String,
    pub headers: Vec<(String, String)>,
}

impl AudiovaultClient {
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be built.
    pub fn new(app_version: &str) -> Result<Self, AudiovaultError> {
        let user_agent = format!("ApricotPlayer/{app_version}");
        let session = Self::build(&user_agent)?;
        Ok(Self {
            user_agent,
            session: Mutex::new(session),
            test_base: test_base_from_environment(),
        })
    }

    fn build(user_agent: &str) -> Result<(Client, Arc<Jar>), AudiovaultError> {
        let jar = Arc::new(Jar::default());
        let client = Client::builder()
            .user_agent(user_agent)
            .cookie_provider(Arc::clone(&jar))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok((client, jar))
    }

    fn session(&self) -> (Client, Arc<Jar>) {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        (session.0.clone(), Arc::clone(&session.1))
    }

    /// Python `audiovault_cookie_jar.clear()`.
    pub fn clear_cookies(&self) {
        if let Ok(session) = Self::build(&self.user_agent) {
            *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
        }
    }

    /// Python `audiovault_request`: only HTTPS `AudioVault` addresses.
    fn request(
        &self,
        url: &str,
        form: Option<&[(&str, &str)]>,
        timeout: Option<Duration>,
        range: Option<String>,
    ) -> Result<Response, AudiovaultError> {
        let parsed = Url::parse(url).map_err(|_| AudiovaultError::UntrustedUrl)?;
        if !is_trusted_url(&parsed) {
            return Err(AudiovaultError::UntrustedUrl);
        }
        let parsed = match &self.test_base {
            Some(base) => Url::parse(&url.replacen(AUDIOVAULT_BASE_URL, base, 1))
                .map_err(|_| AudiovaultError::UntrustedUrl)?,
            None => parsed,
        };
        let (client, _) = self.session();
        let mut request = match form {
            Some(form) => {
                let body = url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs(form)
                    .finish();
                client
                    .post(parsed)
                    .header(
                        reqwest::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .body(body)
            }
            None => client.get(parsed),
        }
        .header(ACCEPT, "text/html,application/octet-stream");
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        let response = request.send()?;
        // `urllib` raises `HTTPError` for every status of 400 and above.
        if response.status().is_client_error() || response.status().is_server_error() {
            let status = response.status();
            return Err(AudiovaultError::message(format!(
                "HTTP Error {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            )));
        }
        Ok(response)
    }

    /// Python `audiovault_login_worker`, first request: the login page whose
    /// form holds the `_token`.
    ///
    /// # Errors
    ///
    /// Returns the transport failure.
    pub fn login_page(&self) -> Result<String, AudiovaultError> {
        let login_url = format!("{AUDIOVAULT_BASE_URL}/login");
        read_page(self.request(&login_url, None, Some(Duration::from_secs(30)), None)?)
    }

    /// Python `audiovault_login_worker`, second request: the filled form.
    ///
    /// # Errors
    ///
    /// Returns `LoginFailed` when `AudioVault` shows the login form again.
    pub fn submit_login(
        &self,
        token: &str,
        email: &str,
        password: &str,
    ) -> Result<(), AudiovaultError> {
        let login_url = format!("{AUDIOVAULT_BASE_URL}/login");
        let response = self.request(
            &login_url,
            Some(&[
                ("_token", token),
                ("email", email),
                ("password", password),
                ("remember", "on"),
            ]),
            Some(Duration::from_secs(30)),
            None,
        )?;
        let final_path = response.url().path().to_owned();
        let page = read_page(response)?;
        if final_path == "/login" || page.contains("name=\"password\"") {
            return Err(AudiovaultError::LoginFailed);
        }
        Ok(())
    }

    /// Python `audiovault_read_authenticated_page`.
    ///
    /// # Errors
    ///
    /// Returns `SessionExpired` for the login page and other failures as is.
    pub fn fetch_page(&self, url: &str) -> Result<String, AudiovaultError> {
        let response = self.request(url, None, Some(Duration::from_secs(30)), None)?;
        let login = response_is_login(&response);
        let page = read_page(response)?;
        if login || page_is_login(&page) {
            return Err(AudiovaultError::SessionExpired);
        }
        Ok(page)
    }

    /// Python `audiovault_archive_size`.
    ///
    /// # Errors
    ///
    /// Returns `RangeUnsupported` when the server ignores the range.
    pub fn archive_size(&self, url: &str) -> Result<u64, AudiovaultError> {
        let mut response = self.request(url, None, None, Some("bytes=0-0".to_owned()))?;
        if content_type(&response).contains("text/html") || response_is_login(&response) {
            return Err(AudiovaultError::SessionExpired);
        }
        let range = header_text(&response, CONTENT_RANGE);
        let size = parse_content_range(&range)
            .filter(|(start, end, _)| *start == 0 && *end == 0)
            .map(|(_, _, size)| size);
        let (Some(size), 206) = (size, response.status().as_u16()) else {
            return Err(AudiovaultError::RangeUnsupported);
        };
        if size == 0 || size > MAX_ARCHIVE {
            return Err(AudiovaultError::ArchiveTooLarge);
        }
        let mut data = Vec::new();
        (&mut response).take(2).read_to_end(&mut data)?;
        if data.len() != 1 {
            return Err(AudiovaultError::message(
                "AudioVault returned an invalid byte range.",
            ));
        }
        Ok(size)
    }

    /// Python `audiovault_read_range`.
    ///
    /// # Errors
    ///
    /// Returns an error for a wrong, incomplete or unsupported range.
    pub fn read_range(&self, url: &str, start: u64, end: u64) -> Result<Vec<u8>, AudiovaultError> {
        if end < start || end - start + 1 > MAX_RANGE_READ {
            return Err(AudiovaultError::message("Invalid AudioVault byte range."));
        }
        let response = self.request(url, None, None, Some(format!("bytes={start}-{end}")))?;
        if content_type(&response).contains("text/html") || response_is_login(&response) {
            return Err(AudiovaultError::SessionExpired);
        }
        let range = header_text(&response, CONTENT_RANGE);
        let (Some((actual_start, actual_end, _)), 206) =
            (parse_content_range(&range), response.status().as_u16())
        else {
            return Err(AudiovaultError::RangeUnsupported);
        };
        if actual_start != start || actual_end > end || actual_end < actual_start {
            return Err(AudiovaultError::message(
                "AudioVault returned the wrong byte range.",
            ));
        }
        let expected = actual_end - actual_start + 1;
        let mut data = Vec::new();
        response.take(expected + 1).read_to_end(&mut data)?;
        if u64::try_from(data.len()).unwrap_or(u64::MAX) != expected {
            return Err(AudiovaultError::message(
                "AudioVault returned an incomplete byte range.",
            ));
        }
        Ok(data)
    }

    /// Python `resolve_audiovault_stream`.
    ///
    /// # Errors
    ///
    /// Returns `SessionExpired` when `AudioVault` answers with a page.
    pub fn resolve_stream(&self, url: &str) -> Result<AudiovaultStream, AudiovaultError> {
        let response = self.request(url, None, None, None)?;
        let final_url = response.url().to_string();
        let content_type = content_type(&response);
        let disposition = header_text(&response, CONTENT_DISPOSITION);
        if content_type.contains("text/html") || response_is_login(&response) {
            return Err(AudiovaultError::SessionExpired);
        }
        let mut headers = vec![
            ("User-Agent".to_owned(), self.user_agent.clone()),
            ("Referer".to_owned(), AUDIOVAULT_BASE_URL.to_owned()),
        ];
        let cookie = self.cookie_header();
        if !cookie.is_empty() {
            headers.push(("Cookie".to_owned(), cookie));
        }
        Ok(AudiovaultStream {
            response,
            final_url,
            content_type,
            disposition,
            headers,
        })
    }

    /// Python `audiovault_cookie_header`.
    pub fn cookie_header(&self) -> String {
        let (_, jar) = self.session();
        let base = self.test_base.as_deref().unwrap_or(AUDIOVAULT_BASE_URL);
        Url::parse(&format!("{base}/"))
            .ok()
            .and_then(|url| jar.cookies(&url))
            .and_then(|value| value.to_str().ok().map(str::to_owned))
            .unwrap_or_default()
    }

    /// Python `audiovault_remote_episode_items` without the item building:
    /// the archive size and its validated members.
    ///
    /// # Errors
    ///
    /// Returns `RangeUnsupported` when only a full download works.
    pub fn remote_archive_entries(
        &self,
        url: &str,
    ) -> Result<(u64, Vec<ZipEntry>), AudiovaultError> {
        let size = self.archive_size(url)?;
        let mut reader = RangeReader::new(size, |start, end| self.read_range(url, start, end));
        let entries =
            zip_archive::read_entries(&mut reader).map_err(|error| reader.error_for(error))?;
        validate_zip_entries(&entries)?;
        Ok((size, entries))
    }

    /// Python `audiovault_remote_episode_worker`: one member read through
    /// ranges into `target` (by way of `target.part`).
    ///
    /// # Errors
    ///
    /// Returns the transport, archive or file error; the partial file is gone.
    pub fn extract_remote_member(
        &self,
        request: &RemoteMemberRequest<'_>,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), AudiovaultError> {
        let temporary = with_name_suffix(request.target, ".part");
        let result = self.extract_remote_member_to(request, &temporary, progress);
        match result {
            Ok(()) => {
                fs::rename(&temporary, request.target)?;
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(error)
            }
        }
    }

    fn extract_remote_member_to(
        &self,
        request: &RemoteMemberRequest<'_>,
        temporary: &Path,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), AudiovaultError> {
        let size = self.archive_size(request.archive_url)?;
        let mut reader = RangeReader::new(size, |start, end| {
            self.read_range(request.archive_url, start, end)
        });
        let entries =
            zip_archive::read_entries(&mut reader).map_err(|error| reader.error_for(error))?;
        validate_zip_entries(&entries)?;
        let entry = entries
            .iter()
            .find(|entry| entry.name == request.member)
            .ok_or_else(|| {
                AudiovaultError::message(format!(
                    "\"There is no item named {:?} in the archive\"",
                    request.member
                ))
            })?
            .clone();
        if request.expected_crc != 0 && entry.crc != request.expected_crc {
            return Err(AudiovaultError::ShowFormat);
        }
        if let Some(parent) = request.target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir_all(request.show_cache_dir)?;
        fs::write(request.show_cache_dir.join(".apricot-partial"), "partial\n")?;
        let _ = fs::remove_file(temporary);
        let mut output = fs::File::create(temporary)?;
        let copied = match zip_archive::open_entry(&mut reader, &entry) {
            Ok(mut source) => {
                copy_with_progress(&mut source, &mut output, entry.file_size, progress)
            }
            Err(error) => Err(error),
        };
        copied.map_err(|error| reader.error_for(error))?;
        output.flush()?;
        Ok(())
    }

    /// Python `audiovault_show_worker`: the whole package into
    /// `<cache>.zip.part`, then extracted into the cache folder.
    ///
    /// # Errors
    ///
    /// Returns the transport, format or extraction failure; the partial
    /// archive is removed.
    pub fn download_show(
        &self,
        url: &str,
        cache_dir: &Path,
        progress: &mut dyn FnMut(ShowProgress),
    ) -> Result<(), AudiovaultError> {
        let archive = show_archive_path(cache_dir);
        let result = self.download_show_archive(url, cache_dir, &archive, progress);
        let _ = fs::remove_file(&archive);
        result
    }

    fn download_show_archive(
        &self,
        url: &str,
        cache_dir: &Path,
        archive: &Path,
        progress: &mut dyn FnMut(ShowProgress),
    ) -> Result<(), AudiovaultError> {
        if let Some(parent) = cache_dir.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut stream = self.resolve_stream(url)?;
        let length = header_u64(&stream.response, CONTENT_LENGTH);
        if length > MAX_ARCHIVE {
            return Err(AudiovaultError::ArchiveTooLarge);
        }
        let mut output = fs::File::create(archive)?;
        let mut buffer = vec![0_u8; CHUNK];
        let mut total = 0_u64;
        let mut last_percent = None;
        loop {
            let read = read_chunk(&mut stream.response, &mut buffer)?;
            if read == 0 {
                break;
            }
            total += u64::try_from(read).unwrap_or_default();
            if total > MAX_ARCHIVE {
                return Err(AudiovaultError::ArchiveTooLarge);
            }
            output.write_all(&buffer[..read])?;
            let percent = (length > 0).then(|| (total * 80 / length).min(80));
            if percent.is_none() || percent != last_percent {
                if percent.is_some() {
                    last_percent = percent;
                }
                progress(ShowProgress::Downloading(percent.unwrap_or(0)));
            }
        }
        output.flush()?;
        drop(output);
        let lowered = stream.disposition.to_lowercase();
        if !stream.content_type.contains("zip")
            && !lowered.contains(".zip")
            && !fs::File::open(archive).is_ok_and(|mut file| zip_archive::is_zip(&mut file))
        {
            return Err(AudiovaultError::ShowFormat);
        }
        safe_extract(archive, cache_dir, &mut |done, total| {
            let extra = (done * 20).checked_div(total).unwrap_or(0);
            progress(ShowProgress::Extracting(80 + extra));
        })?;
        fs::write(cache_dir.join(".apricot-complete"), "complete\n")?;
        let _ = fs::remove_file(cache_dir.join(".apricot-partial"));
        Ok(())
    }

    /// Python `download_audiovault_movie_worker`.
    ///
    /// # Errors
    ///
    /// Returns the transport or file failure.
    pub fn download_movie(
        &self,
        url: &str,
        folder: &Path,
        fallback_name: &str,
        selected_target: Option<&Path>,
    ) -> Result<PathBuf, AudiovaultError> {
        let mut stream = self.resolve_stream(url)?;
        let filename = disposition_filename(&stream.disposition)
            .unwrap_or_else(|| format!("{fallback_name}.mp3"));
        let target = selected_target.map_or_else(
            || {
                let name = Path::new(&filename).file_name().map_or_else(
                    || filename.clone(),
                    |name| name.to_string_lossy().into_owned(),
                );
                folder.join(name)
            },
            Path::to_path_buf,
        );
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(&target)?;
        io::copy(&mut stream.response, &mut output)?;
        output.flush()?;
        Ok(target)
    }
}

/// One member of a remote package, Python's episode item fields.
pub struct RemoteMemberRequest<'a> {
    pub archive_url: &'a str,
    pub member: &'a str,
    pub expected_crc: u32,
    pub target: &'a Path,
    pub show_cache_dir: &'a Path,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShowProgress {
    Downloading(u64),
    Extracting(u64),
}

fn is_trusted_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| TRUSTED_HOSTS.contains(&host.to_ascii_lowercase().as_str()))
}

fn header_text(response: &Response, name: reqwest::header::HeaderName) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn header_u64(response: &Response, name: reqwest::header::HeaderName) -> u64 {
    header_text(response, name)
        .trim()
        .parse()
        .unwrap_or_default()
}

fn content_type(response: &Response) -> String {
    header_text(response, CONTENT_TYPE).to_lowercase()
}

/// Python `audiovault_read_page`.
fn read_page(response: Response) -> Result<String, AudiovaultError> {
    if header_u64(&response, CONTENT_LENGTH) > MAX_RESPONSE {
        return Err(AudiovaultError::message(
            "AudioVault response is unexpectedly large.",
        ));
    }
    let mut data = Vec::new();
    response.take(MAX_RESPONSE + 1).read_to_end(&mut data)?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_RESPONSE {
        return Err(AudiovaultError::message(
            "AudioVault response is unexpectedly large.",
        ));
    }
    Ok(String::from_utf8_lossy(&data).into_owned())
}

/// Python `audiovault_response_is_login` without the page.
fn response_is_login(response: &Response) -> bool {
    let path = response.url().path().trim_end_matches('/');
    path == "/login"
}

fn page_is_login(page: &str) -> bool {
    page.contains("name=\"password\"") || page.contains("name='password'")
}

/// `Content-Range: bytes start-end/size`.
fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim();
    let rest = value
        .get(..5)?
        .eq_ignore_ascii_case("bytes")
        .then(|| &value[5..])?;
    let rest = rest.trim_start();
    if rest.len() == value.len() - 5 {
        // Python's pattern requires whitespace after "bytes".
        return None;
    }
    let (range, size) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let digits = |text: &str| {
        (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse::<u64>().ok())
            .flatten()
    };
    Some((digits(start)?, digits(end)?, digits(size)?))
}

/// Python's `filename\*?=(?:UTF-8''|")?([^";]+)` and `unquote`.
fn disposition_filename(disposition: &str) -> Option<String> {
    let pattern = regex::Regex::new(r#"(?i)filename\*?=(?:UTF-8''|")?([^";]+)"#).ok()?;
    let value = pattern.captures(disposition)?.get(1)?.as_str().trim();
    Some(percent_decode(value))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16)
        {
            output.push(byte);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn read_chunk(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

fn copy_with_progress(
    source: &mut impl Read,
    output: &mut impl Write,
    total: u64,
    progress: &mut dyn FnMut(u64, u64),
) -> io::Result<()> {
    let mut buffer = vec![0_u8; CHUNK];
    let mut done = 0_u64;
    loop {
        let read = read_chunk(source, &mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..read])?;
        done += u64::try_from(read).unwrap_or_default();
        progress(done, total);
    }
}

/// Python `_AudioVaultRangeReader`: a seekable view of the remote package
/// that reads at least 4 MiB at a time and keeps the last block.
struct RangeReader<F> {
    size: u64,
    position: u64,
    cache_start: u64,
    cache: Vec<u8>,
    read_range: F,
    failure: Option<AudiovaultError>,
}

impl<F: FnMut(u64, u64) -> Result<Vec<u8>, AudiovaultError>> RangeReader<F> {
    const fn new(size: u64, read_range: F) -> Self {
        Self {
            size,
            position: 0,
            cache_start: 0,
            cache: Vec::new(),
            read_range,
            failure: None,
        }
    }

    /// The `AudioVault` error behind a failed read, so a session expiry or an
    /// unsupported range keeps its meaning.
    fn error_for(&mut self, error: io::Error) -> AudiovaultError {
        self.failure
            .take()
            .unwrap_or_else(|| AudiovaultError::from(error))
    }
}

impl<F: FnMut(u64, u64) -> Result<Vec<u8>, AudiovaultError>> Read for RangeReader<F> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.size || buffer.is_empty() {
            return Ok(0);
        }
        let requested =
            (self.size - self.position).min(u64::try_from(buffer.len()).unwrap_or(u64::MAX));
        let cache_end = self.cache_start + u64::try_from(self.cache.len()).unwrap_or_default();
        let requested_end = self.position + requested;
        if !(self.cache_start <= self.position && requested_end <= cache_end) {
            let fetch_size = MAX_RANGE_READ.min(requested.max(RANGE_CACHE));
            let fetch_end = (self.size - 1).min(self.position + fetch_size - 1);
            let data = match (self.read_range)(self.position, fetch_end) {
                Ok(data) => data,
                Err(error) => {
                    let message = error.to_string();
                    self.failure = Some(error);
                    return Err(io::Error::other(message));
                }
            };
            let expected_max = fetch_end - self.position + 1;
            if data.is_empty() || u64::try_from(data.len()).unwrap_or(u64::MAX) > expected_max {
                return Err(io::Error::other(
                    "AudioVault returned an invalid byte range.",
                ));
            }
            self.cache_start = self.position;
            self.cache = data;
        }
        let offset = usize::try_from(self.position - self.cache_start).unwrap_or_default();
        let available = self.cache.len() - offset;
        let count = available.min(usize::try_from(requested).unwrap_or(usize::MAX));
        buffer[..count].copy_from_slice(&self.cache[offset..offset + count]);
        self.position += u64::try_from(count).unwrap_or_default();
        Ok(count)
    }
}

impl<F> Seek for RangeReader<F> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.size) + i128::from(offset),
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Negative seek position",
            ));
        }
        self.position = u64::try_from(target).unwrap_or(u64::MAX).min(self.size);
        Ok(self.position)
    }
}

/// Python `validate_audiovault_zip_infos`.
///
/// # Errors
///
/// Returns Python's English message for the first unsafe member.
pub fn validate_zip_entries(entries: &[ZipEntry]) -> Result<(), AudiovaultError> {
    if entries.len() > MAX_ARCHIVE_ENTRIES {
        return Err(AudiovaultError::message(
            "AudioVault archive contains too many entries.",
        ));
    }
    let mut total = 0_u64;
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let name = entry.name.replace('\\', "/");
        let parts = path_parts(&name);
        if name.is_empty()
            || name.starts_with('/')
            || is_absolute_windows(&name)
            || parts.contains(&"..")
            || parts
                .iter()
                .any(|part| part.contains(':') || part.trim_end_matches([' ', '.']) != *part)
        {
            return Err(AudiovaultError::message(
                "Unsafe path in AudioVault archive.",
            ));
        }
        let normalized = name.trim_end_matches('/').to_lowercase();
        if !normalized.is_empty() && !seen.insert(normalized) {
            return Err(AudiovaultError::message(
                "AudioVault archive contains duplicate paths.",
            ));
        }
        if entry.flag_bits & 0x1 != 0 {
            return Err(AudiovaultError::message(
                "Encrypted AudioVault archives are not supported.",
            ));
        }
        let mode = entry.external_attr >> 16;
        if mode != 0 && mode & 0o170_000 == 0o120_000 {
            return Err(AudiovaultError::message(
                "AudioVault archive contains an unsafe link.",
            ));
        }
        total = total.saturating_add(entry.file_size);
        if total > MAX_EXTRACTED {
            return Err(AudiovaultError::message(
                "AudioVault archive expands beyond the safety limit.",
            ));
        }
        if entry.file_size > 1024 * 1024
            && (entry.compress_size == 0
                || entry.file_size / entry.compress_size > 1000
                || (entry.file_size / entry.compress_size == 1000
                    && entry.file_size % entry.compress_size != 0))
        {
            return Err(AudiovaultError::message(
                "AudioVault archive has an unsafe compression ratio.",
            ));
        }
    }
    Ok(())
}

/// `pathlib.Path(name).parts` on Windows, without the separators and `.`.
fn path_parts(name: &str) -> Vec<&str> {
    name.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect()
}

fn is_absolute_windows(name: &str) -> bool {
    let bytes = name.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'))
        || name.starts_with("\\\\")
}

/// Python `path.with_name(path.name + suffix)`.
pub fn with_name_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Python `cache_dir.with_suffix(".zip.part")`.
pub fn show_archive_path(cache_dir: &Path) -> PathBuf {
    let name = cache_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => &name[..index],
        _ => name.as_str(),
    };
    cache_dir.with_file_name(format!("{stem}.zip.part"))
}

/// Python `safe_extract_audiovault_zip`: into `<destination>.extracting`
/// first, then in place of `destination`.
///
/// # Errors
///
/// Returns the archive, validation or file error; the temporary folder is
/// removed.
pub fn safe_extract(
    archive: &Path,
    destination: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), AudiovaultError> {
    let temporary = with_name_suffix(destination, ".extracting");
    let _ = fs::remove_dir_all(&temporary);
    fs::create_dir_all(&temporary)?;
    let result = extract_into(archive, &temporary, progress);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    let _ = fs::remove_dir_all(destination);
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn extract_into(
    archive: &Path,
    temporary: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), AudiovaultError> {
    let mut file = io::BufReader::new(fs::File::open(archive)?);
    let entries = zip_archive::read_entries(&mut file)?;
    validate_zip_entries(&entries)?;
    let total = entries.iter().map(|entry| entry.file_size).sum::<u64>();
    let mut extracted = 0_u64;
    for entry in &entries {
        let relative = path_parts(&entry.name.replace('\\', "/"))
            .into_iter()
            .collect::<PathBuf>();
        let target = temporary.join(&relative);
        if !target.starts_with(temporary) {
            return Err(AudiovaultError::message(
                "Unsafe path in AudioVault archive.",
            ));
        }
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut source = zip_archive::open_entry(&mut file, entry)?;
        let mut output = fs::File::create(&target)?;
        let mut buffer = vec![0_u8; CHUNK];
        loop {
            let read = read_chunk(&mut source, &mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            extracted += u64::try_from(read).unwrap_or_default();
            progress(extracted, total);
        }
    }
    Ok(())
}

/// Python `audiovault_show_cache_is_complete`.
pub fn show_cache_is_complete(cache_dir: &Path) -> bool {
    cache_dir.is_dir()
        && (cache_dir.join(".apricot-complete").exists()
            || !cache_dir.join(".apricot-partial").exists())
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = format!(".{}", extension.to_string_lossy().to_lowercase());
        AUDIO_EXTENSIONS.contains(&extension.as_str())
    })
}

/// Python `audiovault_episode_items` without the item fields: every audio
/// file below `folder`, in natural order of its relative path.
pub fn episode_files(folder: &Path) -> Vec<PathBuf> {
    if !folder.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file() && is_audio_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort_by_cached_key(|path| {
        natural_sort_key(&path.strip_prefix(folder).unwrap_or(path).to_string_lossy())
    });
    files
}

/// One part of Python `natural_sort_key`: `(0, text)` or `(1, number)`.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum NaturalPart {
    Text(String),
    /// Digits without leading zeros, compared by length first.
    Number(usize, String),
}

/// Python `natural_sort_key`: casefolded, the extension set apart with a
/// NUL, then split into text and digit runs.
pub fn natural_sort_key(value: &str) -> Vec<NaturalPart> {
    let mut text = value.to_lowercase();
    if let Some(index) = text.rfind('.') {
        let extension = &text[index + 1..];
        if !extension.is_empty() && !extension.contains(['/', '\\']) {
            text = format!("{}\0{}", &text[..index], extension);
        }
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut digits = false;
    for character in text.chars() {
        let is_digit = character.is_ascii_digit();
        if is_digit != digits {
            push_natural_part(&mut parts, &current, digits);
            current.clear();
            digits = is_digit;
        }
        current.push(character);
    }
    push_natural_part(&mut parts, &current, digits);
    if digits {
        // `re.split` ends with the text after the last number, even empty.
        parts.push(NaturalPart::Text(String::new()));
    }
    parts
}

fn push_natural_part(parts: &mut Vec<NaturalPart>, value: &str, digits: bool) {
    if digits {
        let trimmed = value.trim_start_matches('0');
        parts.push(NaturalPart::Number(trimmed.len(), trimmed.to_owned()));
    } else {
        parts.push(NaturalPart::Text(value.to_owned()));
    }
}

/// Python `audiovault_cache_path_is_link`.
fn is_link(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

/// Python `trim_audiovault_episode_cache`: the oldest cached episodes go
/// until the cache fits, except the protected ones.
pub fn trim_episode_cache(root: &Path, limit_mb: i64, protected: &[PathBuf]) {
    if !root.is_dir() || is_link(root) {
        return;
    }
    let protected = protected
        .iter()
        .map(|path| normalized_path(path))
        .collect::<Vec<_>>();
    let limit = u64::try_from(limit_mb.max(1)).unwrap_or(1) * 1024 * 1024;
    let Ok(show_folders) = fs::read_dir(root) else {
        return;
    };
    let mut candidates = Vec::new();
    let mut total = 0_u64;
    for show_folder in show_folders.flatten().map(|entry| entry.path()) {
        let episode_folder = show_folder.join("_episodes");
        if !show_folder.is_dir()
            || is_link(&show_folder)
            || !episode_folder.is_dir()
            || is_link(&episode_folder)
        {
            continue;
        }
        let Ok(files) = fs::read_dir(&episode_folder) else {
            continue;
        };
        for path in files.flatten().map(|entry| entry.path()) {
            if !path.is_file() || !is_audio_file(&path) || is_link(&path) {
                continue;
            }
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .unwrap_or_default();
            total += metadata.len();
            candidates.push((
                modified,
                path.to_string_lossy().to_lowercase(),
                path,
                metadata.len(),
            ));
        }
    }
    if total <= limit {
        return;
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    for (_, _, path, size) in candidates {
        if total <= limit {
            break;
        }
        if protected.contains(&normalized_path(&path)) {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// Python `os.path.normcase(os.path.abspath(path))`.
fn normalized_path(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase()
}

/// Python `os.utime(target, None)`: a played cached episode counts as new.
pub fn touch(path: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Python `copy_audiovault_show_to_downloads` without the announcement:
/// a chosen folder is merged, the default one replaced.
///
/// # Errors
///
/// Returns the file error.
pub fn copy_show(cache_dir: &Path, target: &Path, merge: bool) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    if target.exists() && !merge {
        fs::remove_dir_all(target)?;
    }
    copy_tree(cache_dir, target)
}

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".apricot-complete" || name == ".apricot-partial" {
            continue;
        }
        let path = entry.path();
        let destination = target.join(&name);
        if path.is_dir() {
            copy_tree(&path, &destination)?;
        } else {
            fs::copy(&path, &destination)?;
        }
    }
    Ok(())
}

/// Python `shutil.copy2` of one cached episode.
///
/// # Errors
///
/// Returns the file error.
pub fn copy_episode(source: &Path, target: &Path) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target).map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::{
        AudiovaultError, RangeReader, disposition_filename, natural_sort_key, parse_content_range,
        safe_extract, show_archive_path, show_cache_is_complete, trim_episode_cache,
        validate_zip_entries, with_name_suffix,
    };
    use crate::zip_archive::{self, ZipEntry, tests::sample_zip};

    fn entry(name: &str) -> ZipEntry {
        ZipEntry {
            name: name.to_owned(),
            flag_bits: 0,
            compress_type: 8,
            crc: 0,
            compress_size: 100,
            file_size: 100,
            header_offset: 0,
            external_attr: 0,
        }
    }

    #[test]
    fn rejects_unsafe_members_like_python() {
        for name in [
            "../evil.mp3",
            "/root.mp3",
            "C:/x.mp3",
            "a:b.mp3",
            "folder /x.mp3",
            "trailing.",
            "",
        ] {
            let error = validate_zip_entries(&[entry(name)]).expect_err(name);
            assert_eq!(
                error.to_string(),
                "Unsafe path in AudioVault archive.",
                "{name}"
            );
        }
        let error = validate_zip_entries(&[entry("A.mp3"), entry("a.MP3")]).expect_err("dup");
        assert_eq!(
            error.to_string(),
            "AudioVault archive contains duplicate paths."
        );
        let mut encrypted = entry("x.mp3");
        encrypted.flag_bits = 1;
        assert!(validate_zip_entries(&[encrypted]).is_err());
        let mut link = entry("x.mp3");
        link.external_attr = 0o120_777 << 16;
        assert!(validate_zip_entries(&[link]).is_err());
        let mut bomb = entry("x.mp3");
        bomb.file_size = 2 * 1024 * 1024 * 1024;
        bomb.compress_size = 1024;
        assert!(validate_zip_entries(&[bomb]).is_err());
        assert!(validate_zip_entries(&[entry("Show/"), entry("Show/01.mp3")]).is_ok());
    }

    #[test]
    fn natural_order_matches_python() {
        let mut names = vec![
            "Episode 10.mp3",
            "Episode 2.mp3",
            "episode 1.mp3",
            "01 intro.mp3",
            "Bonus.mp3",
        ];
        names.sort_by_key(|name| natural_sort_key(name));
        assert_eq!(
            names,
            [
                "01 intro.mp3",
                "Bonus.mp3",
                "episode 1.mp3",
                "Episode 2.mp3",
                "Episode 10.mp3"
            ]
        );
    }

    #[test]
    fn range_reader_caches_blocks_and_reports_the_audiovault_error() {
        let data = (0..=255_u8).cycle().take(10_000).collect::<Vec<_>>();
        let mut calls = 0;
        let mut reader = RangeReader::new(10_000, |start, end| {
            calls += 1;
            Ok(data[usize::try_from(start).unwrap()..=usize::try_from(end).unwrap()].to_vec())
        });
        let mut first = [0_u8; 10];
        reader.read_exact(&mut first).expect("read");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).expect("read");
        assert_eq!(first.len() + rest.len(), 10_000);
        drop(reader);
        assert_eq!(calls, 1);
        let mut failing = RangeReader::new(100, |_, _| {
            Err::<Vec<u8>, _>(AudiovaultError::SessionExpired)
        });
        let error = failing.read(&mut [0_u8; 4]).expect_err("fails");
        assert!(matches!(
            failing.error_for(error),
            AudiovaultError::SessionExpired
        ));
    }

    #[test]
    fn remote_members_are_read_through_ranges() {
        let archive = sample_zip(
            &[
                ("Show/01.mp3", b"first", false),
                ("Show/02.mp3", &b"second ".repeat(300), true),
            ],
            b"",
        );
        let size = u64::try_from(archive.len()).unwrap();
        let mut reader = RangeReader::new(size, |start, end| {
            Ok(archive[usize::try_from(start).unwrap()..=usize::try_from(end).unwrap()].to_vec())
        });
        let entries = zip_archive::read_entries(&mut reader).expect("entries");
        validate_zip_entries(&entries).expect("safe");
        let mut data = Vec::new();
        zip_archive::open_entry(&mut reader, &entries[1])
            .expect("open")
            .read_to_end(&mut data)
            .expect("read");
        assert_eq!(data, b"second ".repeat(300));
        assert!(zip_archive::is_zip(&mut Cursor::new(archive)));
    }

    #[test]
    fn extracts_packages_into_the_cache_folder() {
        let folder = tempfile::tempdir().expect("folder");
        let archive = folder.path().join("123.zip.part");
        std::fs::write(
            &archive,
            sample_zip(
                &[("Season 1/", b"", false), ("Season 1/01.mp3", b"one", true)],
                b"",
            ),
        )
        .expect("archive");
        let destination = folder.path().join("123");
        std::fs::create_dir_all(destination.join("_episodes")).expect("old cache");
        let mut last = (0, 0);
        safe_extract(&archive, &destination, &mut |done, total| {
            last = (done, total);
        })
        .expect("extract");
        assert_eq!(
            std::fs::read(destination.join("Season 1").join("01.mp3")).unwrap(),
            b"one"
        );
        assert!(!destination.join("_episodes").exists());
        assert!(!with_name_suffix(&destination, ".extracting").exists());
        assert_eq!(last, (3, 3));
        assert!(show_cache_is_complete(&destination));
        std::fs::write(destination.join(".apricot-partial"), "partial\n").unwrap();
        assert!(!show_cache_is_complete(&destination));
    }

    #[test]
    fn cache_trimming_removes_the_oldest_unprotected_episodes() {
        let folder = tempfile::tempdir().expect("folder");
        let root = folder.path().join("audiovault");
        let episodes = root.join("7").join("_episodes");
        std::fs::create_dir_all(&episodes).unwrap();
        let old = episodes.join("0001 - a.mp3");
        let protected = episodes.join("0002 - b.mp3");
        let newest = episodes.join("0003 - c.mp3");
        for (index, path) in [&old, &protected, &newest].into_iter().enumerate() {
            std::fs::write(path, vec![0_u8; 400 * 1024]).unwrap();
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(
                std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(1_000 + u64::try_from(index).unwrap()),
            )
            .unwrap();
        }
        trim_episode_cache(&root, 1, std::slice::from_ref(&old));
        assert!(old.exists());
        assert!(!protected.exists());
        assert!(newest.exists());
    }

    #[test]
    fn parses_headers_and_login_forms() {
        assert_eq!(parse_content_range("bytes 0-0/1234"), Some((0, 0, 1234)));
        assert_eq!(parse_content_range("BYTES  5-9/10"), Some((5, 9, 10)));
        assert_eq!(parse_content_range("bytes 0-0/*"), None);
        assert_eq!(parse_content_range("bytes0-0/5"), None);
        assert_eq!(
            disposition_filename("attachment; filename=\"The Movie.mp3\""),
            Some("The Movie.mp3".to_owned())
        );
        assert_eq!(
            disposition_filename("attachment; filename*=UTF-8''Caf%C3%A9.mp3"),
            Some("Café.mp3".to_owned())
        );
        assert_eq!(
            show_archive_path(std::path::Path::new("C:/cache/audiovault/12.5")),
            std::path::Path::new("C:/cache/audiovault/12.zip.part")
        );
        assert_eq!(
            show_archive_path(std::path::Path::new("C:/cache/audiovault/123")),
            std::path::Path::new("C:/cache/audiovault/123.zip.part")
        );
    }
}
