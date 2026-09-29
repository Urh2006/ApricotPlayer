//! Browser side of Python's cookie export: profile folders, browser
//! processes, yt-dlp's `--cookies-from-browser` and the Chromium `DevTools`
//! fallback.

use std::{
    ffi::OsString,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde_json::Value;
use url::Url;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Python `DEVTOOLS_JSON_MAX_BYTES`.
const DEVTOOLS_JSON_MAX_BYTES: u64 = 1_000_000;
/// Python `websockets.connect(max_size=32_000_000)`.
const DEVTOOLS_MESSAGE_MAX_BYTES: usize = 32_000_000;
const YTDLP_EXPORT_TIMEOUT: Duration = Duration::from_secs(180);

/// Python `COOKIES_BROWSER_PROCESS_NAMES`.
#[must_use]
pub fn cookie_browser_process_names(browser: &str) -> &'static [&'static str] {
    match browser.to_lowercase().as_str() {
        "chrome" => &["chrome"],
        "edge" => &["msedge"],
        "brave" => &["brave"],
        "chromium" => &["chromium"],
        "opera" => &["opera"],
        "vivaldi" => &["vivaldi"],
        "firefox" => &["firefox"],
        _ => &[],
    }
}

fn env_path(name: &str, fallback: &str) -> PathBuf {
    std::env::var_os(name).map_or_else(|| PathBuf::from(fallback), PathBuf::from)
}

/// Python `cookie_browser_root`.
#[must_use]
pub fn cookie_browser_root(browser: &str) -> Option<PathBuf> {
    let local = env_path("LOCALAPPDATA", "");
    let roaming = env_path("APPDATA", "");
    Some(match browser.to_lowercase().as_str() {
        "brave" => local.join(r"BraveSoftware\Brave-Browser\User Data"),
        "chrome" => local.join(r"Google\Chrome\User Data"),
        "chromium" => local.join(r"Chromium\User Data"),
        "edge" => local.join(r"Microsoft\Edge\User Data"),
        "vivaldi" => local.join(r"Vivaldi\User Data"),
        "opera" => roaming.join(r"Opera Software\Opera Stable"),
        _ => return None,
    })
}

/// Python `chromium_cookie_file`.
#[must_use]
pub fn chromium_cookie_file(profile: &Path) -> PathBuf {
    let network = profile.join("Network").join("Cookies");
    if network.exists() {
        network
    } else {
        profile.join("Cookies")
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Python `discover_cookie_profiles`: (label, value) pairs.
#[must_use]
pub fn discover_cookie_profiles(browser: &str) -> Vec<(String, String)> {
    let browser = browser.to_lowercase();
    if browser == "firefox" {
        let roots = [
            env_path("APPDATA", "").join(r"Mozilla\Firefox\Profiles"),
            env_path("LOCALAPPDATA", "").join(
                r"Packages\Mozilla.Firefox_n80bbvh6b1yt2\LocalCache\Roaming\Mozilla\Firefox\Profiles",
            ),
        ];
        let mut profiles = Vec::new();
        for root in roots.iter().filter(|root| root.exists()) {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for profile in entries.filter_map(Result::ok).map(|entry| entry.path()) {
                if profile.is_dir() && profile.join("cookies.sqlite").exists() {
                    profiles.push((file_name(&profile), profile.to_string_lossy().into_owned()));
                }
            }
        }
        profiles.sort_by_key(|(label, _)| label.to_lowercase());
        return profiles;
    }
    let Some(root) = cookie_browser_root(&browser).filter(|root| root.exists()) else {
        return Vec::new();
    };
    if browser == "opera" {
        return if chromium_cookie_file(&root).exists() {
            vec![(file_name(&root), root.to_string_lossy().into_owned())]
        } else {
            Vec::new()
        };
    }
    let mut candidates = Vec::new();
    if chromium_cookie_file(&root).exists() {
        candidates.push(root.clone());
    }
    if let Ok(entries) = std::fs::read_dir(&root) {
        candidates.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && chromium_cookie_file(path).exists()),
        );
    }
    candidates.sort_by_key(|path| profile_sort_key(&file_name(path)));
    let mut seen = std::collections::HashSet::new();
    let mut profiles = Vec::new();
    for profile in candidates {
        let value = if profile.parent() == Some(root.as_path()) {
            file_name(&profile)
        } else {
            profile.to_string_lossy().into_owned()
        };
        if seen.insert(value.clone()) {
            profiles.push((file_name(&profile), value));
        }
    }
    profiles
}

/// Python's `sort_key` in `discover_cookie_profiles`.
fn profile_sort_key(name: &str) -> (u8, String) {
    if name == "Default" {
        return (0, name.to_owned());
    }
    if let Some(number) = name
        .strip_prefix("Profile ")
        .filter(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u64>().ok())
    {
        return (1, format!("{number:04}"));
    }
    (2, name.to_lowercase())
}

/// Python `cookie_browser_executable`.
#[must_use]
pub fn cookie_browser_executable(browser: &str) -> Option<PathBuf> {
    let program_files = env_path("ProgramFiles", r"C:\Program Files");
    let program_files_x86 = env_path("ProgramFiles(x86)", r"C:\Program Files (x86)");
    let local = env_path("LOCALAPPDATA", "");
    let candidates: Vec<PathBuf> = match browser.to_lowercase().as_str() {
        "brave" => vec![
            program_files.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            program_files_x86.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            local.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
        ],
        "chrome" => vec![
            program_files.join(r"Google\Chrome\Application\chrome.exe"),
            program_files_x86.join(r"Google\Chrome\Application\chrome.exe"),
            local.join(r"Google\Chrome\Application\chrome.exe"),
        ],
        "edge" => vec![
            program_files_x86.join(r"Microsoft\Edge\Application\msedge.exe"),
            program_files.join(r"Microsoft\Edge\Application\msedge.exe"),
            local.join(r"Microsoft\Edge\Application\msedge.exe"),
        ],
        "chromium" => vec![
            program_files.join(r"Chromium\Application\chrome.exe"),
            program_files_x86.join(r"Chromium\Application\chrome.exe"),
            local.join(r"Chromium\Application\chrome.exe"),
        ],
        "opera" => vec![
            local.join(r"Programs\Opera\opera.exe"),
            program_files.join(r"Opera\opera.exe"),
            program_files_x86.join(r"Opera\opera.exe"),
        ],
        "vivaldi" => vec![
            local.join(r"Vivaldi\Application\vivaldi.exe"),
            program_files.join(r"Vivaldi\Application\vivaldi.exe"),
            program_files_x86.join(r"Vivaldi\Application\vivaldi.exe"),
        ],
        _ => Vec::new(),
    };
    candidates.into_iter().find(|candidate| candidate.exists())
}

/// Python `windows_system_executable`.
fn windows_system_executable(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("WINDIR"))?;
    let path = PathBuf::from(root).join("System32").join(name);
    path.is_file().then_some(path)
}

fn hidden_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// Python `cookie_browser_is_running`.
#[must_use]
pub fn cookie_browser_is_running(browser: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let Some(tasklist) = windows_system_executable("tasklist.exe") else {
        return false;
    };
    cookie_browser_process_names(browser).iter().any(|name| {
        let image = format!("{name}.exe");
        output_with_timeout(
            hidden_command(&tasklist).args(["/FI", &format!("IMAGENAME eq {image}"), "/NH"]),
            Duration::from_secs(2),
        )
        .is_some_and(|(stdout, _)| stdout.to_lowercase().contains(&image.to_lowercase()))
    })
}

/// Python `close_cookie_browser_processes`.
pub fn close_cookie_browser_processes(browser: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let Some(taskkill) = windows_system_executable("taskkill.exe") else {
        return false;
    };
    let names = cookie_browser_process_names(browser);
    for name in names {
        let _ = output_with_timeout(
            hidden_command(&taskkill).args(["/IM", &format!("{name}.exe"), "/T", "/F"]),
            Duration::from_secs(5),
        );
    }
    if names.is_empty() {
        return false;
    }
    std::thread::sleep(Duration::from_secs(1));
    !cookie_browser_is_running(browser)
}

/// Python `wait_for_cookie_browser_exit`.
pub fn wait_for_cookie_browser_exit(browser: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !cookie_browser_is_running(browser) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    !cookie_browser_is_running(browser)
}

/// Runs a short command, killing it after `timeout`.
fn output_with_timeout(command: &mut Command, timeout: Duration) -> Option<(String, String)> {
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let stdout = read(stdout.map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let stderr = read(stderr.map(|pipe| Box::new(pipe) as Box<dyn Read + Send>));
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    Some((stdout.join().ok()?, stderr.join().ok()?))
}

/// yt-dlp's `extract_cookies_from_browser` through the standalone
/// executable: `--cookies-from-browser` loads the browser jar and yt-dlp
/// saves it to `--cookies` on exit, even though no URL was given.
///
/// # Errors
///
/// Returns yt-dlp's error text followed by its warnings, as Python's
/// `cookie_export_error_text` joins the `MemoryYtdlpLogger` summary.
pub fn extract_browser_cookies_with_ytdlp(
    ytdlp: &Path,
    browser: &str,
    profile: Option<&str>,
) -> Result<String, String> {
    let directory = tempfile::Builder::new()
        .prefix("apricot-cookies-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let output = directory.path().join("cookies.txt");
    let specification = profile.map_or_else(
        || browser.to_owned(),
        |profile| format!("{browser}:{profile}"),
    );
    let arguments: [OsString; 6] = [
        "--ignore-config".into(),
        "--no-plugin-dirs".into(),
        "--cookies-from-browser".into(),
        specification.into(),
        "--cookies".into(),
        output.clone().into_os_string(),
    ];
    let (_, stderr) =
        output_with_timeout(hidden_command(ytdlp).args(arguments), YTDLP_EXPORT_TIMEOUT)
            .ok_or_else(|| "yt-dlp could not be started".to_owned())?;
    // yt-dlp logs some problems as errors and still returns the jar (for
    // example a Chromium profile without "Local State"); only a jar that was
    // never saved means the extraction itself failed.
    if let Ok(bytes) = std::fs::read(&output) {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    let mut errors = prefixed_lines(&stderr, "ERROR:");
    let mut summary = prefixed_lines(&stderr, "WARNING:");
    let Some(error) = errors.pop() else {
        return Err(if summary.is_empty() {
            "no cookies found".to_owned()
        } else {
            summary.join("\n")
        });
    };
    errors.append(&mut summary);
    if errors.is_empty() {
        Err(error)
    } else {
        Err(format!("{error}\n{}", errors.join("\n")))
    }
}

fn prefixed_lines(text: &str, prefix: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(prefix))
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Python `chromium_profile_launch_args`: (profile label, arguments).
///
/// # Errors
/// Returns an error when the browser has no profile root.
pub fn chromium_profile_launch_args(
    browser: &str,
    profile: Option<&str>,
    headless: bool,
) -> Result<(String, Vec<String>), String> {
    let root = cookie_browser_root(browser)
        .ok_or_else(|| format!("browser profile root not found for {browser}"))?;
    let profile_value = profile.unwrap_or_default().trim();
    let mut profile_directory = String::new();
    let mut user_data_directory = root.clone();
    if !profile_value.is_empty() && Path::new(profile_value).is_absolute() {
        let profile_path = Path::new(profile_value);
        if profile_path.exists()
            && let Some(parent) = profile_path.parent().filter(|parent| parent.exists())
        {
            parent.clone_into(&mut user_data_directory);
            profile_directory = file_name(profile_path);
        }
    } else if !profile_value.is_empty() {
        profile_value.clone_into(&mut profile_directory);
    } else if browser != "opera" {
        "Default".clone_into(&mut profile_directory);
    }
    let mut arguments = vec![
        format!("--user-data-dir={}", user_data_directory.display()),
        "--remote-allow-origins=*".to_owned(),
        "--disable-gpu".to_owned(),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
        "--disable-background-networking".to_owned(),
        "--disable-features=LockProfileCookieDatabase".to_owned(),
    ];
    if headless {
        arguments.push("--headless=new".to_owned());
    } else {
        arguments.push("--window-position=-32000,-32000".to_owned());
        arguments.push("--window-size=800,600".to_owned());
    }
    if !profile_directory.is_empty() && browser != "opera" {
        arguments.push(format!("--profile-directory={profile_directory}"));
    }
    let label = if profile_directory.is_empty() {
        file_name(&root)
    } else {
        profile_directory
    };
    Ok((label, arguments))
}

/// Python `free_local_port`.
fn free_local_port() -> std::io::Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

/// Python `validate_devtools_websocket_url`.
///
/// # Errors
/// Returns Python's "browser devtools websocket is missing".
pub fn validate_devtools_websocket_url(
    websocket_url: &str,
    expected_port: u16,
) -> Result<Url, String> {
    let missing = || "browser devtools websocket is missing".to_owned();
    let url = Url::parse(websocket_url).map_err(|_| missing())?;
    let host_ok = matches!(
        url.host_str().map(str::to_lowercase).as_deref(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    );
    if !matches!(url.scheme(), "ws" | "wss")
        || !host_ok
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() != Some(expected_port)
    {
        return Err(missing());
    }
    Ok(url)
}

/// Python `fetch_devtools_json`.
fn fetch_devtools_json(port: u16, endpoint: &str, timeout: Duration) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("http://127.0.0.1:{port}{endpoint}"))
        .header(
            reqwest::header::USER_AGENT,
            format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("browser DevTools response {}", response.status()));
    }
    let mut body = Vec::new();
    response
        .take(DEVTOOLS_JSON_MAX_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > DEVTOOLS_JSON_MAX_BYTES {
        return Err("browser DevTools response is too large".to_owned());
    }
    serde_json::from_slice(&body).map_err(|error| error.to_string())
}

/// Python `export_chromium_cookies_via_devtools` up to the cookie list:
/// (profile label, `DevTools` cookies).
///
/// # Errors
/// Returns launch, endpoint or protocol errors.
pub fn devtools_browser_cookies(
    browser: &str,
    profile: Option<&str>,
    headless: bool,
) -> Result<(String, Vec<Value>), String> {
    let executable = cookie_browser_executable(browser)
        .ok_or_else(|| format!("{browser} executable not found"))?;
    let (label, base_arguments) = chromium_profile_launch_args(browser, profile, headless)?;
    let port = free_local_port().map_err(|error| error.to_string())?;
    let mut command = Command::new(&executable);
    command
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--remote-debugging-address=127.0.0.1")
        .args(&base_arguments)
        .arg("https://www.youtube.com/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut process = command.spawn().map_err(|error| error.to_string())?;
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut version = None;
        while Instant::now() < deadline {
            if let Ok(payload) = fetch_devtools_json(port, "/json/version", Duration::from_secs(1))
            {
                version = Some(payload);
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let version =
            version.ok_or_else(|| "browser devtools endpoint did not start".to_owned())?;
        let websocket = version
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let websocket = validate_devtools_websocket_url(websocket, port)?;
        devtools_get_all_cookies(&websocket)
    })();
    let _ = process.kill();
    let _ = process.wait();
    result.map(|cookies| (label, cookies))
}

/// Python `devtools_get_all_cookies`.
fn devtools_get_all_cookies(websocket: &Url) -> Result<Vec<Value>, String> {
    let request = serde_json::json!({
        "id": 1,
        "method": "Network.getCookies",
        "params": {
            "urls": [
                "https://www.youtube.com/",
                "https://music.youtube.com/",
                "https://accounts.google.com/",
                "https://www.google.com/",
                "https://redirector.googlevideo.com/",
            ]
        }
    });
    let mut socket = WebSocket::connect(websocket)?;
    socket.send_text(&request.to_string())?;
    loop {
        let payload: Value =
            serde_json::from_str(&socket.receive_text()?).map_err(|error| error.to_string())?;
        if payload.get("id").and_then(Value::as_i64) != Some(1) {
            continue;
        }
        if let Some(error) = payload.get("error").filter(|error| !error.is_null()) {
            return Err(error.to_string());
        }
        return Ok(payload
            .get("result")
            .and_then(|result| result.get("cookies"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default());
    }
}

/// The small RFC 6455 client `DevTools` needs: one loopback connection, text
/// frames only.
struct WebSocket {
    stream: TcpStream,
}

impl WebSocket {
    fn connect(url: &Url) -> Result<Self, String> {
        if url.scheme() != "ws" {
            return Err("browser devtools websocket is missing".to_owned());
        }
        let host = url
            .host_str()
            .unwrap_or("127.0.0.1")
            .trim_matches(['[', ']']);
        let port = url.port().unwrap_or(80);
        let mut stream = TcpStream::connect((host, port)).map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|error| error.to_string())?;
        let key = base64_encode(&websocket_key_bytes());
        let path = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_owned(),
        };
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        let mut header = Vec::new();
        let mut byte = [0_u8; 1];
        while !header.ends_with(b"\r\n\r\n") {
            if header.len() > 16_384 {
                return Err("browser devtools handshake is too large".to_owned());
            }
            stream
                .read_exact(&mut byte)
                .map_err(|error| error.to_string())?;
            header.push(byte[0]);
        }
        let status = String::from_utf8_lossy(&header);
        if status
            .lines()
            .next()
            .is_none_or(|line| line.split_whitespace().nth(1) != Some("101"))
        {
            return Err("browser devtools websocket handshake failed".to_owned());
        }
        Ok(Self { stream })
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        let mut frame = vec![0x80 | opcode];
        let length = payload.len();
        if length < 126 {
            frame.push(0x80 | u8::try_from(length).unwrap_or_default());
        } else if let Ok(length) = u16::try_from(length) {
            frame.push(0x80 | 0x7e);
            frame.extend_from_slice(&length.to_be_bytes());
        } else {
            frame.push(0x80 | 0x7f);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
        let mask = websocket_mask();
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.stream
            .write_all(&frame)
            .map_err(|error| error.to_string())
    }

    fn send_text(&mut self, text: &str) -> Result<(), String> {
        self.send_frame(0x1, text.as_bytes())
    }

    fn receive_text(&mut self) -> Result<String, String> {
        let mut message = Vec::new();
        loop {
            let mut head = [0_u8; 2];
            self.read(&mut head)?;
            let fin = head[0] & 0x80 != 0;
            let opcode = head[0] & 0x0f;
            let masked = head[1] & 0x80 != 0;
            let mut length = u64::from(head[1] & 0x7f);
            if length == 126 {
                let mut extended = [0_u8; 2];
                self.read(&mut extended)?;
                length = u64::from(u16::from_be_bytes(extended));
            } else if length == 127 {
                let mut extended = [0_u8; 8];
                self.read(&mut extended)?;
                length = u64::from_be_bytes(extended);
            }
            let length = usize::try_from(length)
                .ok()
                .filter(|length| message.len() + length <= DEVTOOLS_MESSAGE_MAX_BYTES)
                .ok_or_else(|| "browser devtools message is too large".to_owned())?;
            let mut mask = [0_u8; 4];
            if masked {
                self.read(&mut mask)?;
            }
            let mut payload = vec![0_u8; length];
            self.read(&mut payload)?;
            if masked {
                for (index, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[index % 4];
                }
            }
            match opcode {
                0x0..=0x2 => {
                    message.extend_from_slice(&payload);
                    if fin {
                        return String::from_utf8(message).map_err(|error| error.to_string());
                    }
                }
                0x8 => return Err("browser devtools websocket closed".to_owned()),
                0x9 => self.send_frame(0xA, &payload)?,
                _ => {}
            }
        }
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<(), String> {
        self.stream
            .read_exact(buffer)
            .map_err(|error| error.to_string())
    }
}

fn websocket_entropy() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos()),
    );
    hasher.finish()
}

fn websocket_key_bytes() -> [u8; 16] {
    let mut key = [0_u8; 16];
    key[..8].copy_from_slice(&websocket_entropy().to_le_bytes());
    key[8..].copy_from_slice(&websocket_entropy().to_le_bytes());
    key
}

fn websocket_mask() -> [u8; 4] {
    let bytes = websocket_entropy().to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for index in 0..4 {
            if index <= chunk.len() {
                text.push(char::from(
                    ALPHABET[((value >> (18 - 6 * index)) & 0x3f) as usize],
                ));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// Python `windows_documents_folders`.
#[must_use]
pub fn windows_documents_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Some(personal) = crate::windows_registration::read_current_user_text(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
        "Personal",
    )
    .filter(|value| !value.is_empty())
    {
        folders.push(PathBuf::from(personal));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        folders.push(PathBuf::from(home).join("Documents"));
    }
    for variable in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(root) = std::env::var_os(variable).filter(|root| !root.is_empty()) {
            folders.push(PathBuf::from(root).join("Documents"));
        }
    }
    let mut seen = std::collections::HashSet::new();
    folders
        .into_iter()
        .filter(|folder| seen.insert(folder.to_string_lossy().to_lowercase()))
        .collect()
}

/// Called by yt-dlp workers when `YouTube` still asks for sign-in with the
/// cookies file, as Python's `repair_cookies_for_error`.
pub trait CookieRepairHook: Send + Sync {
    /// The refreshed cookies file, or `None` when nothing was repaired.
    fn repair(&self, error: &str) -> Option<PathBuf>;
}

static COOKIE_REPAIR_HOOK: RwLock<Option<Arc<dyn CookieRepairHook>>> = RwLock::new(None);

/// Installs the process-wide repair hook the yt-dlp workers use.
pub fn set_cookie_repair_hook(hook: Arc<dyn CookieRepairHook>) {
    if let Ok(mut slot) = COOKIE_REPAIR_HOOK.write() {
        *slot = Some(hook);
    }
}

/// Python `repair_cookies_for_error` through the installed hook.
#[must_use]
pub fn repair_cookies_for_error(error: &str) -> Option<PathBuf> {
    if !apricot_media::cookie_errors::is_cookie_auth_error(error) {
        return None;
    }
    let hook = COOKIE_REPAIR_HOOK.read().ok()?.clone()?;
    hook.repair(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_folders_sort_like_python() {
        let mut names = vec!["Profile 10", "Guest Profile", "Default", "Profile 2"];
        names.sort_by_key(|name| profile_sort_key(name));
        assert_eq!(
            names,
            ["Default", "Profile 2", "Profile 10", "Guest Profile"]
        );
    }

    #[test]
    fn launch_arguments_match_python() {
        let (label, arguments) =
            chromium_profile_launch_args("brave", None, true).expect("arguments");
        assert_eq!(label, "Default");
        assert!(arguments[0].starts_with("--user-data-dir="));
        assert!(arguments[0].ends_with(r"BraveSoftware\Brave-Browser\User Data"));
        assert_eq!(arguments[7], "--headless=new");
        assert_eq!(arguments[8], "--profile-directory=Default");
        let (_, window) =
            chromium_profile_launch_args("edge", Some("Profile 1"), false).expect("arguments");
        assert_eq!(
            &window[7..],
            [
                "--window-position=-32000,-32000",
                "--window-size=800,600",
                "--profile-directory=Profile 1"
            ]
        );
    }

    #[test]
    fn devtools_websocket_must_be_loopback_on_the_launch_port() {
        assert!(
            validate_devtools_websocket_url("ws://127.0.0.1:9222/devtools/browser/x", 9222).is_ok()
        );
        for url in [
            "ws://127.0.0.1:9333/devtools/browser/x",
            "ws://example.com:9222/devtools",
            "http://127.0.0.1:9222/",
            "ws://user@127.0.0.1:9222/",
            "",
        ] {
            assert_eq!(
                validate_devtools_websocket_url(url, 9222).unwrap_err(),
                "browser devtools websocket is missing"
            );
        }
    }

    #[test]
    fn base64_matches_rfc_4648() {
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn websocket_client_reads_the_devtools_cookie_answer() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
        let port = listener.local_addr().expect("address").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("read");
                request.push(byte[0]);
            }
            stream
                .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n")
                .expect("write");
            let mut head = [0_u8; 2];
            stream.read_exact(&mut head).expect("frame");
            assert_eq!(head[0], 0x81);
            let mut length = usize::from(head[1] & 0x7f);
            if length == 126 {
                let mut extended = [0_u8; 2];
                stream.read_exact(&mut extended).expect("length");
                length = usize::from(u16::from_be_bytes(extended));
            }
            let mut mask = [0_u8; 4];
            stream.read_exact(&mut mask).expect("mask");
            let mut payload = vec![0_u8; length];
            stream.read_exact(&mut payload).expect("payload");
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
            let request: Value = serde_json::from_slice(&payload).expect("json");
            assert_eq!(request["method"], "Network.getCookies");
            let event = br#"{"method":"Network.x"}"#;
            let mut frames = vec![0x81, u8::try_from(event.len()).expect("small")];
            frames.extend_from_slice(event);
            let answer = br#"{"id":1,"result":{"cookies":[{"name":"SID","value":"a","domain":".youtube.com"}]}}"#;
            let (first, second) = answer.split_at(10);
            frames.extend([0x01, u8::try_from(first.len()).expect("small")]);
            frames.extend_from_slice(first);
            frames.extend([0x80, u8::try_from(second.len()).expect("small")]);
            frames.extend_from_slice(second);
            stream.write_all(&frames).expect("answer");
        });
        let url = Url::parse(&format!("ws://127.0.0.1:{port}/devtools/browser/x")).expect("url");
        let cookies = devtools_get_all_cookies(&url).expect("cookies");
        server.join().expect("server");
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0]["name"], "SID");
    }

    /// Live check: `APRICOT_YTDLP` is yt-dlp.exe, `APRICOT_FIREFOX_PROFILE` a
    /// folder with a `cookies.sqlite`.
    #[test]
    #[ignore = "needs yt-dlp.exe and a Firefox profile"]
    fn ytdlp_exports_a_firefox_profile() {
        let ytdlp = PathBuf::from(std::env::var_os("APRICOT_YTDLP").expect("APRICOT_YTDLP"));
        let profile = std::env::var("APRICOT_FIREFOX_PROFILE").expect("profile");
        let text =
            extract_browser_cookies_with_ytdlp(&ytdlp, "firefox", Some(&profile)).expect("cookies");
        assert!(text.starts_with("# Netscape HTTP Cookie File"));
        assert!(text.contains("	SID	"));
        let missing = extract_browser_cookies_with_ytdlp(&ytdlp, "firefox", Some("C:\nothing"))
            .expect_err("missing profile");
        assert!(!missing.is_empty());
    }

    #[test]
    fn ytdlp_error_lines_are_collected() {
        let stderr = "Extracting cookies from chrome\nERROR: Could not copy Chrome cookie database. See x\nWARNING: y\n";
        assert_eq!(
            prefixed_lines(stderr, "ERROR:"),
            ["Could not copy Chrome cookie database. See x"]
        );
        assert_eq!(prefixed_lines(stderr, "WARNING:"), ["y"]);
    }
}
