//! Spotify's internal web interfaces (`docs/spotify-p0/P0_EVIDENCE.md` 3):
//! pathfinder GraphQL persisted queries and spclient JSON endpoints, both
//! with the session's login5 token and client token. The public Web API
//! answers 429 for this token, so it is not used.
//!
//! Persisted query hashes are public web-player code. They are read from the
//! web-player bundles once, cached in the app data folder and read again
//! when Spotify reports an unknown hash. No token ever reaches a log.

#![allow(clippy::missing_errors_doc)] // Every call fails only with `ApiError`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use librespot_core::Session;
use serde_json::{Value, json};

const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
const WEB_PLAYER: &str = "https://open.spotify.com/";
const BUNDLE_BASE: &str = "https://open.spotifycdn.com/cdn/build/web-player/";
/// Hashes older than this are read again before use.
const HASHES_MAX_AGE: Duration = Duration::from_hours(7 * 24);
/// The desktop web player; without a desktop browser name Spotify serves the
/// mobile web player, which has other bundles.
pub const BROWSER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("network: {0}")]
    Network(String),
    /// HTTP status other than success, with a short reason.
    #[error("status {0}")]
    Status(u16),
    /// Spotify does not know the operation (changed web player).
    #[error("unknown operation {0}")]
    UnknownOperation(String),
    /// The answer did not have the expected shape.
    #[error("unexpected answer: {0}")]
    Shape(String),
}

impl ApiError {
    /// 401/403: the account may not do this.
    pub const fn is_forbidden(&self) -> bool {
        matches!(self, Self::Status(401 | 403))
    }
}

pub struct Api {
    http: reqwest::Client,
    cache_file: Option<PathBuf>,
    hashes: Mutex<Option<HashCache>>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct HashCache {
    fetched_at: u64,
    hashes: HashMap<String, String>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

/// `"name","query","<64 hex>"` and `"name","mutation","<64 hex>"` entries
/// of the web-player bundles.
pub fn scan_hashes<S: std::hash::BuildHasher>(text: &str, into: &mut HashMap<String, String, S>) {
    for kind in ["\",\"query\",\"", "\",\"mutation\",\""] {
        let mut rest = text;
        while let Some(at) = rest.find(kind) {
            let before = &rest[..at];
            let name_start = before.rfind('"').map_or(0, |start| start + 1);
            let name = &before[name_start..];
            let after = &rest[at + kind.len()..];
            let hash: String = after.chars().take(64).collect();
            if !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && hash.len() == 64
                && hash.chars().all(|c| c.is_ascii_hexdigit())
            {
                into.insert(name.to_owned(), hash);
            }
            rest = &rest[at + kind.len()..];
        }
    }
}

/// The lazily loaded web-player chunks: the bundle builds their names as
/// `(names[id] || id) + "." + hashes[id] + ".js"` from two object literals.
pub fn chunk_files(text: &str) -> Vec<String> {
    let Some(end) = text.find(")[e]+\".js\"") else {
        return Vec::new();
    };
    let Some(start) = text[..end].rfind("=e=>\"\"+") else {
        return Vec::new();
    };
    let segment = &text[start..end];
    let Some(split) = segment.find("+\".\"+") else {
        return Vec::new();
    };
    let pairs = |part: &str| -> Vec<(String, String)> {
        part.split(['{', ','])
            .filter_map(|entry| {
                let (id, value) = entry.split_once(':')?;
                let id = id.trim().trim_matches('"');
                let value = value.trim().trim_end_matches(['}', ')']).trim_matches('"');
                (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()))
                    .then(|| (id.to_owned(), value.to_owned()))
            })
            .collect()
    };
    let names: HashMap<String, String> = pairs(&segment[..split]).into_iter().collect();
    pairs(&segment[split..])
        .into_iter()
        .filter(|(_, hash)| hash.chars().all(|c| c.is_ascii_hexdigit()) && !hash.is_empty())
        .map(|(id, hash)| {
            let name = names.get(&id).cloned().unwrap_or_else(|| id.clone());
            format!("{BUNDLE_BASE}{name}.{hash}.js")
        })
        .collect()
}

impl Api {
    pub fn new(cache_file: Option<PathBuf>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(BROWSER_AGENT)
                .build()
                .unwrap_or_default(),
            cache_file,
            hashes: Mutex::new(None),
        }
    }

    fn cached(&self) -> Option<HashCache> {
        if let Ok(slot) = self.hashes.lock()
            && let Some(cache) = slot.as_ref()
        {
            return Some(cache.clone());
        }
        let file = self.cache_file.as_ref()?;
        let cache: HashCache = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
        if let Ok(mut slot) = self.hashes.lock() {
            *slot = Some(cache.clone());
        }
        Some(cache)
    }

    /// Reads all persisted query hashes from the web-player bundles.
    pub async fn refresh_hashes(&self) -> Result<HashMap<String, String>, ApiError> {
        let network = |error: reqwest::Error| ApiError::Network(error.to_string());
        let html = self
            .http
            .get(WEB_PLAYER)
            .send()
            .await
            .map_err(network)?
            .text()
            .await
            .map_err(network)?;
        let mut scripts: Vec<String> = html
            .split("src=\"")
            .skip(1)
            .filter_map(|part| part.split('"').next())
            .filter(|src| {
                std::path::Path::new(src)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
                    && src.contains("/web-player/")
            })
            .map(str::to_owned)
            .collect();
        let mut hashes = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        while let Some(script) = scripts.pop() {
            if !seen.insert(script.clone()) || seen.len() > 1500 {
                continue;
            }
            let Ok(response) = self.http.get(&script).send().await else {
                continue;
            };
            let Ok(text) = response.text().await else {
                continue;
            };
            scan_hashes(&text, &mut hashes);
            for chunk in chunk_files(&text) {
                if !seen.contains(&chunk) {
                    scripts.push(chunk);
                }
            }
            // Lazily loaded chunks are named in the main bundle.
            for chunk in text.split('"').filter(|part| {
                std::path::Path::new(part)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("js"))
                    && part.len() < 120
                    && !part.contains(' ')
                    && !part.starts_with('/')
            }) {
                let url = if chunk.starts_with("http") {
                    chunk.to_owned()
                } else {
                    format!("{BUNDLE_BASE}{chunk}")
                };
                if !seen.contains(&url) {
                    scripts.push(url);
                }
            }
        }
        if hashes.is_empty() {
            return Err(ApiError::Shape(
                "no persisted queries in the web player".into(),
            ));
        }
        let cache = HashCache {
            fetched_at: now(),
            hashes: hashes.clone(),
        };
        if let Some(file) = &self.cache_file
            && let Ok(bytes) = serde_json::to_vec(&cache)
        {
            let _ = std::fs::write(file, bytes);
        }
        if let Ok(mut slot) = self.hashes.lock() {
            *slot = Some(cache);
        }
        log::info!("pathfinder: {} persisted queries", hashes.len());
        Ok(hashes)
    }

    async fn hash(&self, operation: &str, fresh: bool) -> Result<String, ApiError> {
        let cache = self.cached().filter(|cache| {
            !fresh && now().saturating_sub(cache.fetched_at) < HASHES_MAX_AGE.as_secs()
        });
        let hashes = match cache {
            Some(cache) => cache.hashes,
            None => self.refresh_hashes().await?,
        };
        hashes
            .get(operation)
            .cloned()
            .ok_or_else(|| ApiError::UnknownOperation(operation.to_owned()))
    }

    async fn tokens(session: &Session) -> Result<(String, String), ApiError> {
        let login5 = session
            .login5()
            .auth_token()
            .await
            .map_err(|error| ApiError::Network(error.kind.to_string()))?;
        let client = session
            .spclient()
            .client_token()
            .await
            .map_err(|error| ApiError::Network(error.kind.to_string()))?;
        Ok((login5.access_token, client))
    }

    /// One pathfinder query; `data` of the answer. An unknown hash is read
    /// again from the web player once.
    pub async fn pathfinder(
        &self,
        session: &Session,
        operation: &str,
        variables: Value,
    ) -> Result<Value, ApiError> {
        let mut fresh = false;
        loop {
            let hash = match self.hash(operation, fresh).await {
                Err(ApiError::UnknownOperation(_)) if !fresh => {
                    fresh = true;
                    continue;
                }
                other => other?,
            };
            let body = json!({
                "variables": variables,
                "operationName": operation,
                "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } }
            });
            let answer = self
                .send(
                    session,
                    reqwest::Method::POST,
                    PATHFINDER,
                    Some(body.to_string()),
                )
                .await?;
            let unknown_hash =
                answer
                    .get("errors")
                    .and_then(Value::as_array)
                    .is_some_and(|errors| {
                        errors.iter().any(|error| {
                            error
                                .get("message")
                                .and_then(Value::as_str)
                                .is_some_and(|message| message.contains("PersistedQueryNotFound"))
                        })
                    });
            if unknown_hash && !fresh {
                fresh = true;
                continue;
            }
            if let Some(data) = answer.get("data").filter(|data| !data.is_null()) {
                return Ok(data.clone());
            }
            let message = answer
                .pointer("/errors/0/message")
                .and_then(Value::as_str)
                .unwrap_or("no data");
            return Err(ApiError::Shape(message.chars().take(120).collect()));
        }
    }

    /// A `collection/v2` protobuf request (`write`, `paging`), which has
    /// no JSON form for writing; the answer is read as JSON.
    pub async fn collection(
        &self,
        session: &Session,
        path: &str,
        body: Vec<u8>,
    ) -> Result<Value, ApiError> {
        let base = session
            .spclient()
            .base_url()
            .await
            .map_err(|error| ApiError::Network(error.kind.to_string()))?;
        let (token, client_token) = Self::tokens(session).await?;
        let response = self
            .http
            .post(format!("{base}{path}"))
            .bearer_auth(token)
            .header("client-token", client_token)
            .header(
                "content-type",
                "application/vnd.collection-v2.spotify.proto",
            )
            .header("accept", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(ApiError::Status(status));
        }
        Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// An spclient JSON request; `path` starts with `/`.
    pub async fn spclient(
        &self,
        session: &Session,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ApiError> {
        let base = session
            .spclient()
            .base_url()
            .await
            .map_err(|error| ApiError::Network(error.kind.to_string()))?;
        self.send(
            session,
            method,
            &format!("{base}{path}"),
            body.map(|body| body.to_string()),
        )
        .await
    }

    async fn send(
        &self,
        session: &Session,
        method: reqwest::Method,
        url: &str,
        body: Option<String>,
    ) -> Result<Value, ApiError> {
        let (token, client_token) = Self::tokens(session).await?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(token)
            .header("client-token", client_token)
            .header("accept", "application/json")
            .header("app-platform", "Win32_x86_64");
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json;charset=UTF-8")
                .body(body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        let status = response.status().as_u16();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ApiError::Network(error.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(ApiError::Status(status));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|error| ApiError::Shape(error.to_string()))
    }
}

/// Minimal protobuf writing for `collection2v2` messages.
pub mod proto {
    pub fn varint(buffer: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = u8::try_from(value & 0x7f).unwrap_or(0);
            value >>= 7;
            if value == 0 {
                buffer.push(byte);
                return;
            }
            buffer.push(byte | 0x80);
        }
    }

    pub fn bytes(buffer: &mut Vec<u8>, field: u32, value: &[u8]) {
        varint(buffer, u64::from(field << 3 | 2));
        varint(buffer, value.len() as u64);
        buffer.extend_from_slice(value);
    }

    pub fn int(buffer: &mut Vec<u8>, field: u32, value: u64) {
        varint(buffer, u64::from(field << 3));
        varint(buffer, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_fields_are_tagged_and_length_prefixed() {
        let mut buffer = Vec::new();
        proto::bytes(&mut buffer, 1, b"ab");
        proto::int(&mut buffer, 3, 1);
        proto::int(&mut buffer, 4, 300);
        assert_eq!(buffer, [0x0a, 2, b'a', b'b', 0x18, 1, 0x20, 0xac, 0x02]);
    }

    #[test]
    fn chunk_names_come_from_the_two_maps() {
        let text = r#"u.u=e=>""+(({1328:"xpui-pip",14:"other"})[e]||e)+"."+({1328:"3e036c59",9:"99ea67ed"})[e]+".js",u.miniCssF"#;
        let mut files = chunk_files(text);
        files.sort();
        assert_eq!(
            files,
            [
                format!("{BUNDLE_BASE}9.99ea67ed.js"),
                format!("{BUNDLE_BASE}xpui-pip.3e036c59.js"),
            ]
        );
    }

    #[test]
    fn scans_queries_and_mutations_from_bundle_text() {
        let hash = "a".repeat(64);
        let text = format!(
            r#"x=new t.l("searchDesktop","query","{hash}",null),y=new t.l("addToLibrary","mutation","{hash}",null),z=("bad name","query","{hash}")"#
        );
        let mut hashes = HashMap::new();
        scan_hashes(&text, &mut hashes);
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes["searchDesktop"], hash);
        assert!(hashes.contains_key("addToLibrary"));
    }
}
