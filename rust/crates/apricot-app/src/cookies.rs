//! Python `CookiesUI`: `YouTube` cookie files, their import into the app's
//! `cookies.txt` cache and the browser export algorithm.
//!
//! The browser side (profile folders, processes, yt-dlp and `DevTools`) is a
//! [`BrowserCookieBackend`] so this module stays platform neutral.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

use apricot_core::TranslationCatalog;
use apricot_storage::SettingsDocument;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::comments::friendly_error;

/// Python `COOKIE_PROFILE_AUTO`.
pub const COOKIE_PROFILE_AUTO: &str = "auto";
/// Python `COOKIES_FILE_MAX_BYTES`.
pub const COOKIES_FILE_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Python `COOKIES_BROWSER_OPTIONS`.
pub const COOKIES_BROWSER_OPTIONS: [&str; 8] = [
    "none", "chrome", "edge", "firefox", "brave", "chromium", "opera", "vivaldi",
];
/// Python `CHROMIUM_COOKIE_BROWSERS`.
pub const CHROMIUM_COOKIE_BROWSERS: [&str; 6] =
    ["brave", "chrome", "edge", "chromium", "opera", "vivaldi"];
/// Python `YOUTUBE_COOKIE_DOMAIN_ROOTS`.
pub const YOUTUBE_COOKIE_DOMAIN_ROOTS: [&str; 5] = [
    "google.com",
    "googlevideo.com",
    "youtube.com",
    "youtube-nocookie.com",
    "ytimg.com",
];
/// Python `youtube_auth_cookie_names`.
const YOUTUBE_AUTH_COOKIE_NAMES: [&str; 19] = [
    "sid",
    "sidcc",
    "lsid",
    "osid",
    "hsid",
    "ssid",
    "apisid",
    "sapisid",
    "login_info",
    "account_chooser",
    "__secure-osid",
    "__secure-1psid",
    "__secure-3psid",
    "__secure-1papisid",
    "__secure-3papisid",
    "__secure-1psidcc",
    "__secure-3psidcc",
    "__secure-1psidts",
    "__secure-3psidts",
];
/// `http.cookiejar.MozillaCookieJar` header written by `save`.
const NETSCAPE_HEADER_TEXT: &str = "# Netscape HTTP Cookie File\n\
# http://curl.haxx.se/rfc/cookie_spec.html\n\
# This is a generated file!  Do not edit.\n\n";

/// One cookie as `http.cookiejar.Cookie` stores the fields Apricot uses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cookie {
    pub name: String,
    /// `None` only for a `cookies.txt` line without a name.
    pub value: Option<String>,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    /// Kept as text, as `MozillaCookieJar` round-trips it.
    pub expires: Option<String>,
    pub http_only: bool,
}

/// `http.cookiejar.CookieJar`: one cookie per domain, path and name, iterated
/// in sorted order like `deepvalues`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CookieJar {
    cookies: BTreeMap<(String, String, String), Cookie>,
}

impl CookieJar {
    pub fn set_cookie(&mut self, cookie: Cookie) {
        self.cookies.insert(
            (
                cookie.domain.clone(),
                cookie.path.clone(),
                cookie.name.clone(),
            ),
            cookie,
        );
    }

    pub fn iter(&self) -> impl Iterator<Item = &Cookie> {
        self.cookies.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    /// `MozillaCookieJar.save(ignore_discard=True, ignore_expires=True)`.
    #[must_use]
    pub fn to_netscape_text(&self) -> String {
        let mut text = NETSCAPE_HEADER_TEXT.to_owned();
        for cookie in self.iter() {
            let secure = if cookie.secure { "TRUE" } else { "FALSE" };
            let initial_dot = if cookie.domain.starts_with('.') {
                "TRUE"
            } else {
                "FALSE"
            };
            let (name, value) = cookie.value.as_ref().map_or_else(
                || (String::new(), cookie.name.clone()),
                |value| (cookie.name.clone(), value.clone()),
            );
            let domain = if cookie.http_only {
                format!("#HttpOnly_{}", cookie.domain)
            } else {
                cookie.domain.clone()
            };
            let _ = writeln!(
                text,
                "{domain}\t{initial_dot}\t{}\t{secure}\t{}\t{name}\t{value}",
                cookie.path,
                cookie.expires.as_deref().unwrap_or_default()
            );
        }
        text
    }
}

/// How an imported file was read, for Python's message choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CookieImportKind {
    Json,
    Netscape,
    Header,
}

/// Python `import_cookie_file_to_cache` result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CookieImport {
    pub path: PathBuf,
    pub kind: CookieImportKind,
    pub score: u64,
    pub youtube_count: usize,
    pub total_count: usize,
    pub has_login: bool,
}

impl CookieImport {
    /// Python `choose_cookies_file` message key for the import kind.
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self.kind {
            CookieImportKind::Json => "cookies_file_json_imported",
            CookieImportKind::Header => "cookies_file_header_imported",
            CookieImportKind::Netscape => "cookies_file_netscape_imported",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CookieError {
    /// Python raises `cookies_file_unsupported`.
    Unsupported,
    /// `http.cookiejar.LoadError` text.
    Load(String),
    Io(String),
}

impl CookieError {
    #[must_use]
    pub fn localized(&self, catalog: &TranslationCatalog) -> String {
        match self {
            Self::Unsupported => catalog.text("cookies_file_unsupported").to_owned(),
            Self::Load(text) | Self::Io(text) => text.clone(),
        }
    }
}

impl std::fmt::Display for CookieError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str("cookies file is not in a supported format"),
            Self::Load(text) | Self::Io(text) => formatter.write_str(text),
        }
    }
}

impl std::error::Error for CookieError {}

fn io_error(error: &std::io::Error) -> CookieError {
    CookieError::Io(error.to_string())
}

/// Python `cookie_source_signature`: SHA-256 of the file, as hex.
///
/// # Errors
/// Returns the read error.
pub fn cookie_source_signature(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// Python `Path(os.path.expandvars(str(value).strip('"'))).expanduser()`.
#[must_use]
pub fn expand_cookie_path(value: &str) -> PathBuf {
    let trimmed = value.trim_matches('"');
    let mut expanded = String::new();
    let mut rest = trimmed;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                if let Some(value) = std::env::var_os(name) {
                    expanded.push_str(&value.to_string_lossy());
                } else {
                    expanded.push('%');
                    expanded.push_str(name);
                    expanded.push('%');
                }
                rest = &after[end + 1..];
            }
            _ => {
                expanded.push('%');
                rest = after;
            }
        }
    }
    expanded.push_str(rest);
    if let Some(stripped) = expanded
        .strip_prefix("~\\")
        .or_else(|| expanded.strip_prefix("~/"))
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        return PathBuf::from(home).join(stripped);
    }
    if expanded == "~"
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        return PathBuf::from(home);
    }
    PathBuf::from(expanded)
}

/// Python `paths_match`.
#[must_use]
pub fn paths_match(first: &Path, second: &Path) -> bool {
    let first = expand_cookie_path(&first.to_string_lossy());
    let second = expand_cookie_path(&second.to_string_lossy());
    match (fs::canonicalize(&first), fs::canonicalize(&second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => normalized_absolute(&first) == normalized_absolute(&second),
    }
}

fn normalized_absolute(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_owned(), |dir| dir.join(path))
    };
    absolute
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Python `decode_cookie_file_bytes`.
#[must_use]
pub fn decode_cookie_file_bytes(data: &[u8]) -> String {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_owned();
    }
    let (text, _, had_errors) = encoding_rs::WINDOWS_1252.decode(data);
    if had_errors {
        String::from_utf8_lossy(data).into_owned()
    } else {
        text.into_owned()
    }
}

/// Python `looks_like_netscape_cookie_text`.
#[must_use]
pub fn looks_like_netscape_cookie_text(text: &str) -> bool {
    let head = text.chars().take(500).collect::<String>().to_lowercase();
    if head.contains("# netscape http cookie file") || head.contains("# http cookie file") {
        return true;
    }
    for raw_line in text_lines(text) {
        let line = raw_line.trim();
        if line.is_empty() || (line.starts_with('#') && !line.starts_with("#HttpOnly_")) {
            continue;
        }
        if line.split('\t').count() >= 7 || split_whitespace_max(line, 6).len() >= 7 {
            return true;
        }
    }
    false
}

/// Python `normalized_netscape_cookie_text`.
#[must_use]
pub fn normalized_netscape_cookie_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut has_header = false;
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    for raw_line in unified.split('\n') {
        let mut line = raw_line.trim_start_matches('\u{feff}').to_owned();
        let lowered = line.to_lowercase();
        if lowered.starts_with("# netscape http cookie file")
            || lowered.starts_with("# http cookie file")
        {
            has_header = true;
        }
        let stripped = line.trim();
        if !stripped.is_empty() && !stripped.starts_with('#') && !stripped.contains('\t') {
            let parts = split_whitespace_max(stripped, 6);
            if parts.len() >= 7 {
                line = parts[..7].join("\t");
            }
        }
        lines.push(line);
    }
    if !has_header {
        lines.insert(0, "# Netscape HTTP Cookie File".to_owned());
        lines.insert(1, "# This file was normalized by ApricotPlayer.".to_owned());
    }
    format!("{}\n", lines.join("\n").trim_end())
}

/// `MozillaCookieJar.load(ignore_discard=True, ignore_expires=True)`.
///
/// # Errors
/// Returns `LoadError` text for a missing header or a malformed line.
pub fn parse_netscape_cookie_text(text: &str, filename: &str) -> Result<CookieJar, CookieError> {
    let mut lines = text_lines(text);
    let magic = lines.next().unwrap_or_default();
    let magic_found = magic
        .find("# HTTP Cookie File")
        .or_else(|| magic.find("# Netscape HTTP Cookie File"))
        .is_some();
    if !magic_found {
        return Err(CookieError::Load(format!(
            "'{filename}' does not look like a Netscape format cookies file"
        )));
    }
    let mut jar = CookieJar::default();
    for raw_line in lines {
        let (http_only, line) = raw_line
            .strip_prefix("#HttpOnly_")
            .map_or((false, raw_line), |rest| (true, rest));
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with('$') || trimmed.is_empty() {
            continue;
        }
        let invalid = || {
            CookieError::Load(format!(
                "invalid Netscape format cookies file '{filename}': '{raw_line}'"
            ))
        };
        let fields = line.split('\t').collect::<Vec<_>>();
        let [domain, domain_specified, path, secure, expires, name, value] = fields[..] else {
            return Err(invalid());
        };
        if (domain_specified == "TRUE") != domain.starts_with('.') {
            return Err(invalid());
        }
        let (name, value) = if name.is_empty() {
            (value.to_owned(), None)
        } else {
            (name.to_owned(), Some(value.to_owned()))
        };
        jar.set_cookie(Cookie {
            name,
            value,
            domain: domain.to_owned(),
            path: path.to_owned(),
            secure: secure == "TRUE",
            expires: (!expires.is_empty()).then(|| expires.to_owned()),
            http_only,
        });
    }
    Ok(jar)
}

/// Python universal-newline line iteration.
fn text_lines(text: &str) -> impl Iterator<Item = &str> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    text.split('\n')
        .flat_map(|line| line.split('\r'))
        .filter(|_| !text.is_empty())
}

/// Python `re.split(r"\s+", line, maxsplit=count)` on a stripped line.
fn split_whitespace_max(line: &str, count: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = line;
    while parts.len() < count {
        let Some(start) = rest.find(char::is_whitespace) else {
            break;
        };
        parts.push(rest[..start].to_owned());
        rest = rest[start..].trim_start();
    }
    parts.push(rest.to_owned());
    parts
}

/// Python `str(value)` for JSON cookie fields.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.as_i64().map_or_else(
            || {
                number.as_f64().map_or_else(
                    || number.to_string(),
                    |float| {
                        if float.fract() == 0.0 && float.abs() < 1e16 {
                            format!("{float:.1}")
                        } else {
                            float.to_string()
                        }
                    },
                )
            },
            |integer| integer.to_string(),
        ),
        other => other.to_string(),
    }
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python `a or b or c`: the first truthy value.
fn first_truthy<'a>(item: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|key| item.get(*key))
        .find(|value| python_truthy(value))
}

/// Python `cookie_bool`.
fn cookie_bool(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|float| float != 0.0),
        Some(value) if python_truthy(value) => matches!(
            python_str(value).trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        ),
        _ => false,
    }
}

/// Python `cookie_expiry`.
fn cookie_expiry(value: Option<&Value>) -> Option<i64> {
    let float = match value? {
        Value::Bool(true) => 1.0,
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => {
            if text.is_empty() || text == "-1" || text == "0" {
                return None;
            }
            text.trim().parse::<f64>().ok()?
        }
        _ => return None,
    };
    // Python's -1 and 0 checks are covered by the positive check below.
    if !float.is_finite() {
        return None;
    }
    let seconds = if float > 10_000_000_000.0 {
        float / 1000.0
    } else {
        float
    };
    #[allow(clippy::cast_possible_truncation)]
    (seconds > 0.0).then_some(seconds.trunc() as i64)
}

/// Python `cookie_default_domain_from_text`.
fn cookie_default_domain_from_text(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(index) = text.find("://") {
        let after = &text[index + 3..];
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        return after[..end].to_owned();
    }
    text.split('/').next().unwrap_or_default().to_owned()
}

/// Python `looks_like_cookie_domain_key`.
fn looks_like_cookie_domain_key(key: &str) -> bool {
    let key = key.trim();
    if key.is_empty() || key.chars().count() > 120 || key.contains(' ') {
        return false;
    }
    let key = key.strip_prefix('.').unwrap_or(key);
    key.contains('.') && !key.contains('/') && !key.contains('\\')
}

/// Python `cookie_fields_are_safe`.
#[must_use]
pub fn cookie_fields_are_safe(name: &str, value: &str, domain: &str, path: &str) -> bool {
    [name, value, domain, path]
        .iter()
        .all(|field| !field.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}'))
}

/// Python `cookie_from_mapping`.
#[must_use]
pub fn cookie_from_mapping(item: &Map<String, Value>, default_domain: &str) -> Option<Cookie> {
    let name = first_truthy(item, &["name", "Name", "key"])
        .map(python_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if name.is_empty() {
        return None;
    }
    let value = item
        .get("value")
        .filter(|value| !value.is_null())
        .or_else(|| item.get("Value").filter(|value| !value.is_null()))
        .map_or_else(String::new, python_str);
    let mut domain = first_truthy(item, &["domain", "Domain", "host", "host_key", "hostKey"])
        .map_or_else(|| default_domain.to_owned(), python_str)
        .trim()
        .to_owned();
    if domain.starts_with("#HttpOnly_") {
        domain.drain(.."#HttpOnly_".len());
    }
    if domain.contains("://") {
        domain = cookie_default_domain_from_text(&domain);
    }
    if domain.is_empty() {
        return None;
    }
    let path = first_truthy(item, &["path", "Path"]).map_or_else(|| "/".to_owned(), python_str);
    if !cookie_fields_are_safe(&name, &value, &domain, &path) {
        return None;
    }
    let expires = [
        "expirationDate",
        "expiration_date",
        "expires",
        "expiry",
        "expiration",
        "Expiry",
    ]
    .iter()
    .find(|key| item.contains_key(**key))
    .and_then(|key| cookie_expiry(item.get(*key)));
    let http_only = if item.contains_key("httpOnly") {
        cookie_bool(item.get("httpOnly"))
    } else {
        cookie_bool(item.get("http_only"))
    };
    Some(Cookie {
        name,
        value: Some(value),
        domain,
        path: if path.is_empty() {
            "/".to_owned()
        } else {
            path
        },
        secure: cookie_bool(item.get("secure")),
        expires: expires.map(|seconds| seconds.to_string()),
        http_only,
    })
}

/// Python `iter_cookie_json_items`.
fn collect_cookie_json_items<'a>(
    data: &'a Value,
    default_domain: &str,
    items: &mut Vec<(&'a Map<String, Value>, String)>,
) {
    match data {
        Value::Array(values) => {
            for value in values {
                collect_cookie_json_items(value, default_domain, items);
            }
        }
        Value::Object(map) => {
            let own = first_truthy(map, &["url", "host", "domain"])
                .map(|value| cookie_default_domain_from_text(&python_str(value)))
                .filter(|domain| !domain.is_empty())
                .unwrap_or_else(|| default_domain.to_owned());
            if ["name", "Name", "key"]
                .iter()
                .any(|key| map.contains_key(*key))
                && ["value", "Value"].iter().any(|key| map.contains_key(*key))
            {
                items.push((map, own.clone()));
            }
            for (key, value) in map {
                let child = if looks_like_cookie_domain_key(key) {
                    key.as_str()
                } else {
                    own.as_str()
                };
                if value.is_array() || value.is_object() {
                    collect_cookie_json_items(value, child, items);
                }
            }
        }
        _ => {}
    }
}

/// Python `cookie_jar_from_json_data`.
#[must_use]
pub fn cookie_jar_from_json_data(data: &Value) -> CookieJar {
    let mut items = Vec::new();
    collect_cookie_json_items(data, "", &mut items);
    let mut jar = CookieJar::default();
    let mut seen = HashSet::new();
    for (item, default_domain) in items {
        let Some(cookie) = cookie_from_mapping(item, &default_domain) else {
            continue;
        };
        if seen.insert((
            cookie.domain.clone(),
            cookie.path.clone(),
            cookie.name.clone(),
        )) {
            jar.set_cookie(cookie);
        }
    }
    jar
}

/// Python `cookie_jar_from_header_text`.
///
/// # Errors
/// Returns [`CookieError::Unsupported`] when the text is no cookie header.
pub fn cookie_jar_from_header_text(text: &str) -> Result<CookieJar, CookieError> {
    let mut combined = text_lines(text)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    if combined.is_empty() {
        return Err(CookieError::Unsupported);
    }
    if combined.to_lowercase().starts_with("cookie:") {
        combined = combined
            .split_once(':')
            .map(|(_, rest)| rest.trim().to_owned())
            .unwrap_or_default();
    }
    if !combined.contains('=') || !combined.contains(';') {
        return Err(CookieError::Unsupported);
    }
    let ignored = [
        "path", "expires", "max-age", "secure", "httponly", "samesite", "domain", "priority",
    ];
    let mut jar = CookieJar::default();
    for part in combined.split(';') {
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || ignored.contains(&name.to_lowercase().as_str()) {
            continue;
        }
        let mut item = Map::new();
        item.insert("name".to_owned(), Value::String(name.to_owned()));
        item.insert("value".to_owned(), Value::String(value.trim().to_owned()));
        item.insert(
            "domain".to_owned(),
            Value::String(".youtube.com".to_owned()),
        );
        item.insert("path".to_owned(), Value::String("/".to_owned()));
        if let Some(cookie) = cookie_from_mapping(&item, "") {
            jar.set_cookie(cookie);
        }
    }
    Ok(jar)
}

/// Python `cookie_domain_matches`.
#[must_use]
pub fn cookie_domain_matches(domain: &str, root: &str) -> bool {
    let normalize = |value: &str| {
        value
            .trim()
            .to_lowercase()
            .trim_start_matches('.')
            .trim_end_matches('.')
            .to_owned()
    };
    let host = normalize(domain);
    let root = normalize(root);
    !host.is_empty() && !root.is_empty() && (host == root || host.ends_with(&format!(".{root}")))
}

/// Python `cookie_domain_is_youtube_related`.
#[must_use]
pub fn cookie_domain_is_youtube_related(domain: &str) -> bool {
    YOUTUBE_COOKIE_DOMAIN_ROOTS
        .iter()
        .any(|root| cookie_domain_matches(domain, root))
}

/// Python `youtube_cookie_jar`.
#[must_use]
pub fn youtube_cookie_jar(jar: &CookieJar) -> CookieJar {
    let mut filtered = CookieJar::default();
    for cookie in jar.iter() {
        let path = if cookie.path.is_empty() {
            "/"
        } else {
            &cookie.path
        };
        if cookie_domain_is_youtube_related(&cookie.domain)
            && cookie_fields_are_safe(
                &cookie.name,
                cookie.value.as_deref().unwrap_or_default(),
                &cookie.domain,
                path,
            )
        {
            filtered.set_cookie(cookie.clone());
        }
    }
    filtered
}

/// Python `cdp_cookies_to_cookie_jar`.
#[must_use]
pub fn cdp_cookies_to_cookie_jar(cookies: &[Value]) -> CookieJar {
    let mut jar = CookieJar::default();
    for item in cookies.iter().filter_map(Value::as_object) {
        if let Some(cookie) = cookie_from_mapping(item, "")
            && cookie_domain_is_youtube_related(&cookie.domain)
        {
            jar.set_cookie(cookie);
        }
    }
    jar
}

fn is_auth_cookie_name(name: &str) -> bool {
    YOUTUBE_AUTH_COOKIE_NAMES.contains(&name.to_lowercase().as_str())
}

/// Python `cookie_jar_has_login_cookies`.
#[must_use]
pub fn cookie_jar_has_login_cookies(jar: &CookieJar) -> bool {
    jar.iter().any(|cookie| {
        let domain = cookie.domain.to_lowercase();
        (cookie_domain_matches(&domain, "google.com")
            || cookie_domain_matches(&domain, "youtube.com"))
            && is_auth_cookie_name(&cookie.name)
    })
}

/// Python `cookie_jar_score`: (score, `YouTube` cookies, all cookies).
#[must_use]
pub fn cookie_jar_score(jar: &CookieJar) -> (u64, usize, usize) {
    let mut score = 0;
    let mut youtube_count = 0;
    let mut total_count = 0;
    for cookie in jar.iter() {
        total_count += 1;
        let domain = cookie.domain.to_lowercase();
        let is_youtube = cookie_domain_matches(&domain, "youtube.com");
        let is_google = cookie_domain_matches(&domain, "google.com");
        if is_youtube {
            youtube_count += 1;
            score += 3;
        }
        if is_google || is_youtube {
            score += 1;
            if is_auth_cookie_name(&cookie.name) {
                score += 100;
            }
        }
    }
    (score, youtube_count, total_count)
}

/// Python `cookie_score_summary`.
#[must_use]
pub fn cookie_score_summary(label: &str, jar: &CookieJar) -> String {
    let (score, youtube_count, total_count) = cookie_jar_score(jar);
    let login = if cookie_jar_has_login_cookies(jar) {
        "yes"
    } else {
        "no"
    };
    format!(
        "{label}: {total_count} cookies, {youtube_count} YouTube cookies, login cookies {login}, score {score}"
    )
}

/// Python `cookie_file_score`: (score, `YouTube` cookies, all cookies, login).
///
/// # Errors
/// Returns read or `LoadError` text.
pub fn cookie_file_score(path: &Path) -> Result<(u64, usize, usize, bool), CookieError> {
    let bytes = fs::read(path).map_err(|error| io_error(&error))?;
    let jar =
        parse_netscape_cookie_text(&decode_cookie_file_bytes(&bytes), &path.to_string_lossy())?;
    let (score, youtube_count, total_count) = cookie_jar_score(&jar);
    Ok((
        score,
        youtube_count,
        total_count,
        cookie_jar_has_login_cookies(&jar),
    ))
}

/// Python `cookies_file_has_youtube_login`.
#[must_use]
pub fn cookies_file_has_youtube_login(path: &Path) -> bool {
    cookie_file_score(path).is_ok_and(|(_, _, _, has_login)| has_login)
}

/// Python `save_cookie_jar_to_cache`: only `YouTube` cookies, replaced
/// atomically.
///
/// # Errors
/// Returns the write error.
pub fn save_cookie_jar_to_cache(jar: &CookieJar, cache: &Path) -> Result<(), CookieError> {
    let text = youtube_cookie_jar(jar).to_netscape_text();
    let directory = cache.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory).map_err(|error| io_error(&error))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.subsec_nanos());
    let temporary = directory.join(format!(".cookies-save-{}-{nanos}.tmp", std::process::id()));
    let result = fs::write(&temporary, text).and_then(|()| fs::rename(&temporary, cache));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| io_error(&error))
}

/// Python `import_cookie_file_to_cache`.
///
/// # Errors
/// Returns read, format or write errors.
pub fn import_cookie_file_to_cache(
    source: &Path,
    cache: &Path,
) -> Result<CookieImport, CookieError> {
    let size = fs::metadata(source)
        .map_err(|error| io_error(&error))?
        .len();
    if size > COOKIES_FILE_MAX_BYTES {
        return Err(CookieError::Unsupported);
    }
    let text = decode_cookie_file_bytes(&fs::read(source).map_err(|error| io_error(&error))?);
    let stripped = text.trim_start();
    let mut parsed = None;
    if (stripped.starts_with('{') || stripped.starts_with('['))
        && let Ok(data) = serde_json::from_str::<Value>(&text)
    {
        parsed = Some((cookie_jar_from_json_data(&data), CookieImportKind::Json));
    }
    let (jar, kind) = match parsed {
        Some(parsed) => parsed,
        None if looks_like_netscape_cookie_text(&text) => (
            parse_netscape_cookie_text(
                &normalized_netscape_cookie_text(&text),
                &cache
                    .with_file_name(".cookies-import.tmp")
                    .to_string_lossy(),
            )?,
            CookieImportKind::Netscape,
        ),
        None => (
            cookie_jar_from_header_text(&text)?,
            CookieImportKind::Header,
        ),
    };
    let jar = youtube_cookie_jar(&jar);
    if jar.is_empty() {
        return Err(CookieError::Unsupported);
    }
    save_cookie_jar_to_cache(&jar, cache)?;
    let (score, youtube_count, total_count) = cookie_jar_score(&jar);
    Ok(CookieImport {
        path: cache.to_owned(),
        kind,
        score,
        youtube_count,
        total_count,
        has_login: cookie_jar_has_login_cookies(&jar),
    })
}

/// Python `normalized_cookies_browser`: empty for "none".
#[must_use]
pub fn normalized_cookies_browser(settings: &SettingsDocument) -> String {
    let browser = settings.cookies_from_browser.trim().to_lowercase();
    if browser.is_empty() || browser == "none" {
        String::new()
    } else {
        browser
    }
}

/// Python `remember_cookie_source`.
pub fn remember_cookie_source(settings: &mut SettingsDocument, source: &str, imported: &Path) {
    let source = expand_cookie_path(source);
    settings.cookies_source_file = source.to_string_lossy().into_owned();
    settings.cookies_source_signature = cookie_source_signature(&source).unwrap_or_default();
    settings.cookies_file = imported.to_string_lossy().into_owned();
    "none".clone_into(&mut settings.cookies_from_browser);
    COOKIE_PROFILE_AUTO.clone_into(&mut settings.cookies_browser_profile);
}

/// Python `finish_browser_cookies_export` and the tail of
/// `export_browser_cookies_blocking`.
pub fn remember_browser_export(settings: &mut SettingsDocument, cache: &Path, browser: &str) {
    settings.cookies_file = cache.to_string_lossy().into_owned();
    settings.cookies_source_file.clear();
    settings.cookies_source_signature.clear();
    browser.clone_into(&mut settings.cookies_from_browser);
}

/// Python `configured_cookies_display_path` after the legacy migration ran.
#[must_use]
pub fn configured_cookies_display_path(settings: &SettingsDocument) -> String {
    let source = settings.cookies_source_file.trim();
    if source.is_empty() {
        settings.cookies_file.trim().to_owned()
    } else {
        source.to_owned()
    }
}

/// Python `apply_settings_from_visible_controls` for the cookies path text.
pub fn apply_cookies_path_text(settings: &mut SettingsDocument, entered: &str) {
    let entered = entered.trim();
    if entered != configured_cookies_display_path(settings) {
        entered.clone_into(&mut settings.cookies_source_file);
        settings.cookies_source_signature.clear();
        entered.clone_into(&mut settings.cookies_file);
    }
}

fn cache_ready(cache: &Path) -> bool {
    fs::metadata(cache).is_ok_and(|metadata| metadata.len() > 0)
}

/// Python `discover_legacy_cookie_source`.
#[must_use]
pub fn discover_legacy_cookie_source(
    settings: &SettingsDocument,
    cache: &Path,
    documents_folders: &[PathBuf],
) -> Option<PathBuf> {
    if !settings.cookies_source_file.trim().is_empty()
        || !normalized_cookies_browser(settings).is_empty()
    {
        return None;
    }
    let configured = settings.cookies_file.trim();
    if configured.is_empty() || !paths_match(Path::new(configured), cache) {
        return None;
    }
    let cache_name = cache.file_name()?.to_string_lossy().into_owned();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for folder in documents_folders.iter().filter(|folder| folder.is_dir()) {
        let exact = folder.join(&cache_name);
        if exact.is_file() {
            candidates.push(exact);
        }
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        let mut matches = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path.file_name().is_some_and(|name| {
                        name.to_string_lossy().to_lowercase().contains("cookies")
                            && path
                                .extension()
                                .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
                    })
            })
            .collect::<Vec<_>>();
        matches.sort();
        for path in matches {
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let (_, _, total, _) = cookie_file_score(&candidate).ok()?;
            if total == 0 {
                return None;
            }
            let exact = candidate.file_name().is_some_and(|name| {
                name.to_string_lossy().to_lowercase() == cache_name.to_lowercase()
            });
            let modified = fs::metadata(&candidate).ok()?.modified().ok()?;
            Some((exact, modified, candidate))
        })
        .max_by(|first, second| (first.0, first.1).cmp(&(second.0, second.1)))
        .map(|(_, _, path)| path)
}

/// Python `migrate_legacy_cookie_source`: the migrated source path, after the
/// settings were updated, or `None`.
pub fn migrate_legacy_cookie_source(
    settings: &mut SettingsDocument,
    cache: &Path,
    documents_folders: &[PathBuf],
) -> Option<String> {
    let source = discover_legacy_cookie_source(settings, cache, documents_folders)?;
    let result = import_cookie_file_to_cache(&source, cache).ok()?;
    let source = source.to_string_lossy().into_owned();
    remember_cookie_source(settings, &source, &result.path);
    Some(source)
}

/// Result of Python `effective_cookies_file`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectiveCookies {
    /// The cookies file for yt-dlp, or empty.
    pub path: String,
    /// The settings changed and must be saved.
    pub settings_changed: bool,
    /// Python `cookie_source_refresh_error`.
    pub refresh_error: String,
}

/// Python `effective_cookies_file`.
pub fn effective_cookies_file(
    settings: &mut SettingsDocument,
    cache: &Path,
    documents_folders: &[PathBuf],
    catalog: &TranslationCatalog,
) -> EffectiveCookies {
    let configured = settings.cookies_file.trim().to_owned();
    let mut source_value = settings.cookies_source_file.trim().to_owned();
    let mut result = EffectiveCookies::default();
    if source_value.is_empty()
        && let Some(migrated) = migrate_legacy_cookie_source(settings, cache, documents_folders)
    {
        source_value = migrated;
        result.settings_changed = true;
    }
    if !source_value.is_empty() {
        let source_path = expand_cookie_path(&source_value);
        let signature = match cookie_source_signature(&source_path) {
            Ok(signature) => signature,
            Err(error) => {
                result.refresh_error = error.to_string();
                String::new()
            }
        };
        let ready = cache_ready(cache);
        if !signature.is_empty() && (signature != settings.cookies_source_signature || !ready) {
            match import_cookie_file_to_cache(&source_path, cache) {
                Ok(import) => {
                    remember_cookie_source(settings, &source_path.to_string_lossy(), &import.path);
                    result.settings_changed = true;
                    result.path = import.path.to_string_lossy().into_owned();
                    return result;
                }
                Err(error) => {
                    result.refresh_error = friendly_error(catalog, &error.localized(catalog));
                }
            }
        }
        if ready {
            result.path = cache.to_string_lossy().into_owned();
            return result;
        }
    }
    if !configured.is_empty() {
        let configured_path = expand_cookie_path(&configured);
        let same_as_cache = match (fs::canonicalize(&configured_path), fs::canonicalize(cache)) {
            (Ok(first), Ok(second)) => first == second,
            _ => false,
        };
        if !same_as_cache
            && configured_path.exists()
            && let Ok(import) = import_cookie_file_to_cache(&configured_path, cache)
        {
            remember_cookie_source(settings, &configured_path.to_string_lossy(), &import.path);
            result.settings_changed = true;
            result.path = import.path.to_string_lossy().into_owned();
            return result;
        }
        result.path = configured_path.to_string_lossy().into_owned();
        return result;
    }
    if cache_ready(cache) {
        result.path = cache.to_string_lossy().into_owned();
    }
    result
}

/// Python `cookie_profile_choice_values`.
#[must_use]
pub fn cookie_profile_choice_values(
    discovered: &[(String, String)],
    selected: &str,
) -> Vec<String> {
    let mut values = vec![COOKIE_PROFILE_AUTO.to_owned()];
    values.extend(discovered.iter().map(|(_, value)| value.clone()));
    let selected = selected.trim();
    let selected = if selected.is_empty() {
        COOKIE_PROFILE_AUTO
    } else {
        selected
    };
    if !values.iter().any(|value| value == selected) {
        values.push(selected.to_owned());
    }
    values
}

fn is_absolute_profile(value: &str) -> bool {
    Path::new(value).is_absolute() || value.starts_with('\\') || value.starts_with('/')
}

fn profile_name(value: &str) -> String {
    Path::new(value).file_name().map_or_else(
        || value.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Python `cookie_profile_choice_labels`.
#[must_use]
pub fn cookie_profile_choice_label(catalog: &TranslationCatalog, value: &str) -> String {
    if value == COOKIE_PROFILE_AUTO {
        catalog.text("browser_profile_auto").to_owned()
    } else if is_absolute_profile(value) {
        profile_name(value)
    } else {
        value.to_owned()
    }
}

/// Python `cookie_profile_candidates`: (label, profile) with `None` for
/// yt-dlp's own profile choice.
#[must_use]
pub fn cookie_profile_candidates(
    discovered: &[(String, String)],
    selected: &str,
    auto_label: &str,
) -> Vec<(String, Option<String>)> {
    let selected = selected.trim();
    let mut candidates = Vec::new();
    if !selected.is_empty() && selected != COOKIE_PROFILE_AUTO {
        let label = if is_absolute_profile(selected) {
            profile_name(selected)
        } else {
            selected.to_owned()
        };
        candidates.push((label, Some(selected.to_owned())));
    }
    candidates.extend(
        discovered
            .iter()
            .map(|(label, value)| (label.clone(), Some(value.clone()))),
    );
    candidates.push((auto_label.to_owned(), None));
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|(_, profile)| seen.insert(profile.clone().unwrap_or_default()))
        .collect()
}

/// The platform half of Python's browser cookie export.
pub trait BrowserCookieBackend {
    /// Whether the bundled yt-dlp exists (Python `get_yt_dlp()`).
    fn ytdlp_available(&self) -> bool;
    /// Python `discover_cookie_profiles`: (label, value).
    fn discover_profiles(&self, browser: &str) -> Vec<(String, String)>;
    /// Python `cookie_browser_is_running`.
    fn is_running(&self, browser: &str) -> bool;
    /// Python `close_cookie_browser_processes`.
    fn close(&self, browser: &str) -> bool;
    /// Python `wait_for_cookie_browser_exit`.
    fn wait_for_exit(&self, browser: &str, timeout: Duration) -> bool;
    fn sleep(&self, duration: Duration);
    /// yt-dlp `extract_cookies_from_browser`: `cookies.txt` text, or the error
    /// text with yt-dlp's warnings.
    ///
    /// # Errors
    /// Returns the extraction error text.
    fn extract(&self, browser: &str, profile: Option<&str>) -> Result<String, String>;
    /// Python `export_chromium_cookies_via_devtools` before the cookie check:
    /// the profile label and the `DevTools` cookies.
    ///
    /// # Errors
    /// Returns the launch or protocol error text.
    fn devtools_cookies(
        &self,
        browser: &str,
        profile: Option<&str>,
        headless: bool,
    ) -> Result<(String, Vec<Value>), String>;
}

/// Successful browser export: the jar to cache and the profile it came from.
#[derive(Clone, Debug)]
pub struct BrowserCookieExport {
    pub jar: CookieJar,
    pub profile_label: String,
}

struct BestExport {
    score: u64,
    label: String,
    jar: CookieJar,
}

fn attempt_failed(catalog: &TranslationCatalog, profile: &str, error: &str) -> String {
    catalog
        .text("cookie_profile_attempt_failed")
        .replace("{profile}", profile)
        .replace("{error}", error)
}

/// Python `export_browser_cookies_blocking` without the final save.
///
/// # Errors
/// Returns Python's localized failure text with the diagnostics.
#[allow(clippy::too_many_lines)]
pub fn export_browser_cookies(
    backend: &dyn BrowserCookieBackend,
    catalog: &TranslationCatalog,
    browser: &str,
    selected_profile: &str,
    allow_close: bool,
) -> Result<BrowserCookieExport, String> {
    if !backend.ytdlp_available() {
        return Err(catalog.text("missing_ytdlp").to_owned());
    }
    if allow_close && backend.is_running(browser) {
        backend.close(browser);
        backend.wait_for_exit(browser, Duration::from_secs(6));
    }
    let candidates = cookie_profile_candidates(
        &backend.discover_profiles(browser),
        selected_profile,
        catalog.text("browser_profile_auto"),
    );
    let mut errors: Vec<String> = Vec::new();
    let mut best: Option<BestExport> = None;
    let mut copy_lock_error_seen = false;
    for attempt in 0..2 {
        let mut lock_error_seen = false;
        for (label, profile) in &candidates {
            match backend.extract(browser, profile.as_deref()) {
                Ok(text) => {
                    let jar = match parse_netscape_cookie_text(&text, "cookies.txt") {
                        Ok(jar) => jar,
                        Err(error) => {
                            errors.push(attempt_failed(
                                catalog,
                                label,
                                &friendly_error(catalog, &error.to_string()),
                            ));
                            continue;
                        }
                    };
                    let (score, youtube_count, total_count) = cookie_jar_score(&jar);
                    if total_count == 0 {
                        errors.push(attempt_failed(catalog, label, "no cookies found"));
                        continue;
                    }
                    errors.push(cookie_score_summary(label, &jar));
                    if best.as_ref().is_none_or(|best| score > best.score) {
                        best = Some(BestExport {
                            score,
                            label: label.clone(),
                            jar,
                        });
                    }
                    if score >= 100 && youtube_count > 0 {
                        break;
                    }
                }
                Err(error) => {
                    let text = friendly_error(catalog, &error);
                    let lowered = text.to_lowercase();
                    if lowered.contains("could not copy") && lowered.contains("cookie") {
                        lock_error_seen = true;
                        copy_lock_error_seen = true;
                    }
                    errors.push(attempt_failed(catalog, label, &text));
                }
            }
        }
        if best.as_ref().is_some_and(|best| best.score > 0) {
            break;
        }
        if allow_close && lock_error_seen && attempt == 0 {
            backend.close(browser);
            backend.wait_for_exit(browser, Duration::from_secs(8));
            backend.sleep(Duration::from_secs(1));
            continue;
        }
        break;
    }
    let needs_devtools = copy_lock_error_seen
        || best
            .as_ref()
            .is_none_or(|best| !cookie_jar_has_login_cookies(&best.jar));
    if allow_close
        && CHROMIUM_COOKIE_BROWSERS.contains(&browser)
        && browser != "chrome"
        && needs_devtools
    {
        backend.close(browser);
        backend.wait_for_exit(browser, Duration::from_secs(8));
        let mut tried = HashSet::new();
        for (label, profile) in &candidates {
            if !tried.insert(profile.clone().unwrap_or_else(|| "Default".to_owned())) {
                continue;
            }
            for headless in [true, false] {
                let mode = if headless {
                    "DevTools headless"
                } else {
                    "DevTools window"
                };
                let exported = backend
                    .devtools_cookies(browser, profile.as_deref(), headless)
                    .and_then(|(cdp_label, cookies)| {
                        let jar = cdp_cookies_to_cookie_jar(&cookies);
                        let (score, _, total_count) = cookie_jar_score(&jar);
                        if total_count == 0 || score == 0 || !cookie_jar_has_login_cookies(&jar) {
                            Err(catalog.text("browser_cookies_no_youtube").to_owned())
                        } else {
                            Ok((cdp_label, jar))
                        }
                    });
                match exported {
                    Ok((cdp_label, jar)) => {
                        let (score, youtube_count, _) = cookie_jar_score(&jar);
                        let used_label = if cdp_label.is_empty() {
                            label.clone()
                        } else {
                            cdp_label
                        };
                        errors.push(cookie_score_summary(&format!("{used_label} {mode}"), &jar));
                        if best.as_ref().is_none_or(|best| score > best.score) {
                            best = Some(BestExport {
                                score,
                                label: used_label,
                                jar,
                            });
                        }
                        if score >= 100 && youtube_count > 0 {
                            break;
                        }
                    }
                    Err(error) => errors.push(attempt_failed(
                        catalog,
                        &format!("{label} {mode}"),
                        &friendly_error(catalog, &error),
                    )),
                }
            }
            if best
                .as_ref()
                .is_some_and(|best| best.score >= 100 && cookie_jar_has_login_cookies(&best.jar))
            {
                break;
            }
        }
    }
    match best {
        Some(best) if best.score > 0 && cookie_jar_has_login_cookies(&best.jar) => {
            Ok(BrowserCookieExport {
                jar: best.jar,
                profile_label: best.label,
            })
        }
        best => {
            let mut details = if errors.is_empty() {
                vec![catalog.text("cookie_all_profiles_failed").to_owned()]
            } else {
                errors[errors.len().saturating_sub(10)..].to_vec()
            };
            if let Some(best) = best {
                details.push(format!(
                    "Best profile was {}, but it did not contain usable Google/YouTube login cookies.",
                    best.label
                ));
            }
            Err(format!(
                "{}\n\n{}",
                catalog.text("browser_cookies_no_youtube"),
                catalog
                    .text("cookie_export_diagnostics")
                    .replace("{details}", &details.join("\n"))
            ))
        }
    }
}

/// Python `str.title()` for the browser name in messages.
#[must_use]
pub fn browser_title(browser: &str) -> String {
    let mut title = String::with_capacity(browser.len());
    let mut start = true;
    for ch in browser.chars() {
        if ch.is_alphabetic() {
            if start {
                title.extend(ch.to_uppercase());
            } else {
                title.extend(ch.to_lowercase());
            }
            start = false;
        } else {
            title.push(ch);
            start = true;
        }
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn catalog() -> TranslationCatalog {
        crate::embedded_catalog("en")
    }

    #[test]
    fn netscape_round_trip_matches_mozilla_cookie_jar() {
        let text = "# Netscape HTTP Cookie File\n\
            .youtube.com\tTRUE\t/\tTRUE\t1793280126\tSID\tabc\n\
            #HttpOnly_.google.com\tTRUE\t/\tFALSE\t\tHSID\tx\n\
            www.youtube.com\tFALSE\t/\tFALSE\t0\t\tnameless\n";
        let jar = parse_netscape_cookie_text(text, "c.txt").expect("parse");
        assert_eq!(jar.len(), 3);
        assert_eq!(
            jar.to_netscape_text(),
            "# Netscape HTTP Cookie File\n\
             # http://curl.haxx.se/rfc/cookie_spec.html\n\
             # This is a generated file!  Do not edit.\n\n\
             #HttpOnly_.google.com\tTRUE\t/\tFALSE\t\tHSID\tx\n\
             .youtube.com\tTRUE\t/\tTRUE\t1793280126\tSID\tabc\n\
             www.youtube.com\tFALSE\t/\tFALSE\t0\t\tnameless\n"
        );
    }

    #[test]
    fn netscape_loader_rejects_what_python_rejects() {
        assert!(matches!(
            parse_netscape_cookie_text(".a.com\tTRUE\t/\tFALSE\t0\tn\tv\n", "x"),
            Err(CookieError::Load(message)) if message.contains("does not look like")
        ));
        assert!(matches!(
            parse_netscape_cookie_text("# HTTP Cookie File\n.a.com\tFALSE\t/\tFALSE\t0\tn\tv\n", "x"),
            Err(CookieError::Load(message)) if message.contains("invalid Netscape")
        ));
    }

    #[test]
    fn space_separated_lines_are_normalized_with_a_header() {
        let text = ".youtube.com TRUE / TRUE 0 SID value with space\n";
        assert!(looks_like_netscape_cookie_text(text));
        assert_eq!(
            normalized_netscape_cookie_text(text),
            "# Netscape HTTP Cookie File\n# This file was normalized by ApricotPlayer.\n\
             .youtube.com\tTRUE\t/\tTRUE\t0\tSID\tvalue with space\n"
        );
    }

    #[test]
    fn json_exports_are_read_like_python() {
        let data = serde_json::json!({
            "www.youtube.com": [
                {"name": "SID", "value": "a", "expirationDate": 1_793_280_126_500_i64, "secure": true, "httpOnly": "yes"},
                {"name": "PREF", "value": 5},
            ],
            "cookies": [{"Name": "LOGIN_INFO", "Value": "b", "domain": ".youtube.com", "path": ""}]
        });
        let jar = cookie_jar_from_json_data(&data);
        let cookies = jar.iter().collect::<Vec<_>>();
        assert_eq!(cookies.len(), 3);
        let sid = cookies
            .iter()
            .find(|cookie| cookie.name == "SID")
            .expect("sid");
        assert_eq!(sid.domain, "www.youtube.com");
        assert_eq!(sid.expires.as_deref(), Some("1793280126"));
        assert!(sid.secure && sid.http_only);
        let pref = cookies
            .iter()
            .find(|cookie| cookie.name == "PREF")
            .expect("pref");
        assert_eq!(pref.value.as_deref(), Some("5"));
        assert_eq!(pref.expires, None);
        let login = cookies
            .iter()
            .find(|cookie| cookie.name == "LOGIN_INFO")
            .expect("login");
        assert_eq!(login.path, "/");
    }

    #[test]
    fn header_text_becomes_youtube_cookies() {
        let jar = cookie_jar_from_header_text("Cookie: SID=abc; Path=/; HSID=x ;").expect("jar");
        let names = jar
            .iter()
            .map(|cookie| cookie.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["HSID", "SID"]);
        assert!(jar.iter().all(|cookie| cookie.domain == ".youtube.com"));
        assert_eq!(
            cookie_jar_from_header_text("SID=abc"),
            Err(CookieError::Unsupported)
        );
    }

    #[test]
    fn scores_and_login_detection_match_python() {
        let text = "# HTTP Cookie File\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tSID\tx\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n\
            .google.com\tTRUE\t/\tTRUE\t0\tNID\tx\n\
            .example.com\tTRUE\t/\tTRUE\t0\tSID\tx\n";
        let jar = parse_netscape_cookie_text(text, "x").expect("jar");
        assert_eq!(cookie_jar_score(&jar), (4 + 100 + 4 + 1, 2, 4));
        assert!(cookie_jar_has_login_cookies(&jar));
        let filtered = youtube_cookie_jar(&jar);
        assert_eq!(filtered.len(), 3);
        assert_eq!(
            cookie_score_summary("Default", &filtered),
            "Default: 3 cookies, 2 YouTube cookies, login cookies yes, score 109"
        );
    }

    #[test]
    fn import_writes_only_youtube_cookies_to_the_cache() {
        let directory = tempfile::tempdir().expect("dir");
        let source = directory.path().join("export.json");
        fs::write(
            &source,
            r#"[{"domain":".youtube.com","name":"SID","value":"a"},{"domain":".example.com","name":"x","value":"y"}]"#,
        )
        .expect("write");
        let cache = directory.path().join("app").join("cookies.txt");
        let result = import_cookie_file_to_cache(&source, &cache).expect("import");
        assert_eq!(result.kind, CookieImportKind::Json);
        assert!(result.has_login);
        assert_eq!(result.total_count, 1);
        let written = fs::read_to_string(&cache).expect("cache");
        assert!(written.ends_with(".youtube.com\tTRUE\t/\tFALSE\t\tSID\ta\n"));
        assert!(!written.contains("example"));
        let empty = directory.path().join("other.txt");
        fs::write(
            &empty,
            "# HTTP Cookie File\n.example.com\tTRUE\t/\tFALSE\t0\tx\ty\n",
        )
        .expect("write");
        assert_eq!(
            import_cookie_file_to_cache(&empty, &cache),
            Err(CookieError::Unsupported)
        );
    }

    #[test]
    fn effective_file_reimports_a_changed_source() {
        let directory = tempfile::tempdir().expect("dir");
        let source = directory.path().join("mine.txt");
        let cache = directory.path().join("cookies.txt");
        fs::write(&source, "SID=a; HSID=b").expect("write");
        let mut settings = SettingsDocument {
            cookies_source_file: source.to_string_lossy().into_owned(),
            ..SettingsDocument::default()
        };
        let effective = effective_cookies_file(&mut settings, &cache, &[], &catalog());
        assert!(effective.settings_changed);
        assert_eq!(effective.path, cache.to_string_lossy());
        assert_eq!(settings.cookies_file, cache.to_string_lossy());
        assert_eq!(settings.cookies_source_signature.len(), 64);
        let unchanged = effective_cookies_file(&mut settings, &cache, &[], &catalog());
        assert!(!unchanged.settings_changed);
        assert_eq!(unchanged.path, cache.to_string_lossy());
        fs::write(&source, "not cookies").expect("write");
        let failed = effective_cookies_file(&mut settings, &cache, &[], &catalog());
        assert_eq!(failed.path, cache.to_string_lossy());
        assert_eq!(
            failed.refresh_error,
            catalog().text("cookies_file_unsupported")
        );
    }

    #[test]
    fn a_configured_python_cache_is_copied_into_the_beta_cache() {
        let directory = tempfile::tempdir().expect("dir");
        let python_cache = directory.path().join("python-cookies.txt");
        fs::write(
            &python_cache,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tx\n",
        )
        .expect("write");
        let cache = directory.path().join("beta").join("cookies.txt");
        let mut settings = SettingsDocument {
            cookies_file: python_cache.to_string_lossy().into_owned(),
            ..SettingsDocument::default()
        };
        let effective = effective_cookies_file(&mut settings, &cache, &[], &catalog());
        assert_eq!(effective.path, cache.to_string_lossy());
        assert_eq!(settings.cookies_source_file, python_cache.to_string_lossy());
        assert_eq!(
            fs::read_to_string(&python_cache).expect("python"),
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tx\n"
        );
    }

    #[test]
    fn legacy_documents_cookies_are_migrated() {
        let directory = tempfile::tempdir().expect("dir");
        let documents = directory.path().join("Documents");
        fs::create_dir_all(&documents).expect("dir");
        fs::write(
            documents.join("youtube-cookies.txt"),
            "# HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tx\n",
        )
        .expect("write");
        let cache = directory.path().join("cookies.txt");
        let mut settings = SettingsDocument {
            cookies_file: cache.to_string_lossy().into_owned(),
            ..SettingsDocument::default()
        };
        let effective = effective_cookies_file(
            &mut settings,
            &cache,
            std::slice::from_ref(&documents),
            &catalog(),
        );
        assert!(effective.settings_changed);
        assert_eq!(
            settings.cookies_source_file,
            documents.join("youtube-cookies.txt").to_string_lossy()
        );
    }

    #[test]
    fn typed_path_replaces_the_source_only_when_changed() {
        let mut settings = SettingsDocument {
            cookies_source_file: "C:\\a.txt".to_owned(),
            cookies_source_signature: "sig".to_owned(),
            ..SettingsDocument::default()
        };
        apply_cookies_path_text(&mut settings, "C:\\a.txt");
        assert_eq!(settings.cookies_source_signature, "sig");
        apply_cookies_path_text(&mut settings, " C:\\b.txt ");
        assert_eq!(settings.cookies_source_file, "C:\\b.txt");
        assert_eq!(settings.cookies_file, "C:\\b.txt");
        assert!(settings.cookies_source_signature.is_empty());
    }

    #[test]
    fn profile_choices_and_candidates_follow_python() {
        let discovered = vec![
            ("Default".to_owned(), "Default".to_owned()),
            ("Profile 1".to_owned(), "Profile 1".to_owned()),
        ];
        assert_eq!(
            cookie_profile_choice_values(&discovered, "C:\\x\\Work"),
            ["auto", "Default", "Profile 1", "C:\\x\\Work"]
        );
        assert_eq!(
            cookie_profile_choice_label(&catalog(), "C:\\x\\Work"),
            "Work"
        );
        assert_eq!(
            cookie_profile_candidates(&discovered, "Profile 1", "Auto"),
            [
                ("Profile 1".to_owned(), Some("Profile 1".to_owned())),
                ("Default".to_owned(), Some("Default".to_owned())),
                ("Auto".to_owned(), None),
            ]
        );
    }

    struct FakeBackend {
        extract: Vec<Result<String, String>>,
        devtools: Vec<Result<(String, Vec<Value>), String>>,
        calls: RefCell<Vec<String>>,
    }

    impl BrowserCookieBackend for FakeBackend {
        fn ytdlp_available(&self) -> bool {
            true
        }
        fn discover_profiles(&self, _browser: &str) -> Vec<(String, String)> {
            vec![("Default".to_owned(), "Default".to_owned())]
        }
        fn is_running(&self, _browser: &str) -> bool {
            false
        }
        fn close(&self, browser: &str) -> bool {
            self.calls.borrow_mut().push(format!("close {browser}"));
            true
        }
        fn wait_for_exit(&self, _browser: &str, _timeout: Duration) -> bool {
            true
        }
        fn sleep(&self, _duration: Duration) {}
        fn extract(&self, _browser: &str, profile: Option<&str>) -> Result<String, String> {
            let index = self
                .calls
                .borrow()
                .iter()
                .filter(|call| call.starts_with("extract"))
                .count();
            self.calls
                .borrow_mut()
                .push(format!("extract {}", profile.unwrap_or("auto")));
            self.extract
                .get(index)
                .cloned()
                .unwrap_or_else(|| Err("gone".to_owned()))
        }
        fn devtools_cookies(
            &self,
            _browser: &str,
            profile: Option<&str>,
            headless: bool,
        ) -> Result<(String, Vec<Value>), String> {
            let index = self
                .calls
                .borrow()
                .iter()
                .filter(|call| call.starts_with("devtools"))
                .count();
            self.calls.borrow_mut().push(format!(
                "devtools {} {headless}",
                profile.unwrap_or("Default")
            ));
            self.devtools
                .get(index)
                .cloned()
                .unwrap_or_else(|| Err("no devtools".to_owned()))
        }
    }

    #[test]
    fn export_stops_at_the_first_profile_with_login_cookies() {
        let backend = FakeBackend {
            extract: vec![Ok(
                "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tx\n".to_owned(),
            )],
            devtools: Vec::new(),
            calls: RefCell::new(Vec::new()),
        };
        let export =
            export_browser_cookies(&backend, &catalog(), "brave", "auto", true).expect("export");
        assert_eq!(export.profile_label, "Default");
        assert_eq!(*backend.calls.borrow(), ["extract Default"]);
    }

    #[test]
    fn a_locked_database_retries_then_falls_back_to_devtools() {
        let locked = Err("Could not copy Chrome cookie database".to_owned());
        let backend = FakeBackend {
            extract: vec![locked.clone(), locked.clone(), locked.clone(), locked],
            devtools: vec![Ok((
                "Default".to_owned(),
                vec![serde_json::json!({"name": "SID", "value": "x", "domain": ".youtube.com"})],
            ))],
            calls: RefCell::new(Vec::new()),
        };
        let export =
            export_browser_cookies(&backend, &catalog(), "edge", "auto", true).expect("export");
        assert_eq!(export.profile_label, "Default");
        assert_eq!(
            *backend.calls.borrow(),
            [
                "extract Default",
                "extract auto",
                "close edge",
                "extract Default",
                "extract auto",
                "close edge",
                "devtools Default true",
            ]
        );
    }

    #[test]
    fn export_failure_lists_the_attempts() {
        let backend = FakeBackend {
            extract: vec![
                Ok(
                    "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tx\n"
                        .to_owned(),
                ),
                Err("boom".to_owned()),
            ],
            devtools: Vec::new(),
            calls: RefCell::new(Vec::new()),
        };
        let error = export_browser_cookies(&backend, &catalog(), "chrome", "auto", true)
            .expect_err("no login");
        let catalog = catalog();
        assert!(error.starts_with(catalog.text("browser_cookies_no_youtube")));
        assert!(error.contains("Default: 1 cookies, 1 YouTube cookies, login cookies no, score 4"));
        assert!(error.contains(
            "Best profile was Default, but it did not contain usable Google/YouTube login cookies."
        ));
    }

    #[test]
    fn browser_titles_match_python_title() {
        assert_eq!(browser_title("brave"), "Brave");
        assert_eq!(browser_title("edge"), "Edge");
    }
}
