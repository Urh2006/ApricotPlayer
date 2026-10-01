//! Browser login: OAuth authorization code with PKCE and a loopback redirect
//! on `127.0.0.1`, with state check, cancel and deadline.
//!
//! The client ID is `LibreSpot`'s own (`SessionConfig::default().client_id`);
//! Apricot never embeds or asks for one (plan D03, P0 evidence 2).

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use rand::RngCore;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

/// Scopes requested by the upstream `librespot --enable-oauth` binary.
pub const SCOPES: &[&str] = &[
    "app-remote-control",
    "playlist-modify",
    "playlist-modify-private",
    "playlist-modify-public",
    "playlist-read",
    "playlist-read-collaborative",
    "playlist-read-private",
    "streaming",
    "ugc-image-upload",
    "user-follow-modify",
    "user-follow-read",
    "user-library-modify",
    "user-library-read",
    "user-modify",
    "user-modify-playback-state",
    "user-modify-private",
    "user-personalized",
    "user-read-birthdate",
    "user-read-currently-playing",
    "user-read-email",
    "user-read-play-history",
    "user-read-playback-position",
    "user-read-playback-state",
    "user-read-private",
    "user-read-recently-played",
    "user-top-read",
];

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OAuthError {
    #[error("login cancelled")]
    Cancelled,
    #[error("login timed out")]
    TimedOut,
    #[error("login was denied in the browser")]
    Denied,
    #[error("the browser returned an unexpected answer")]
    BadCallback,
    #[error("network error: {0}")]
    Network(String),
}

/// One pending browser login.
pub struct PkceLogin {
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    verifier: String,
    auth_url: String,
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let symbols = chunk.len() + 1;
        for index in 0..symbols {
            out.push(char::from(
                ALPHABET[((value >> (18 - 6 * index)) & 63) as usize],
            ));
        }
    }
    out
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut buffer);
    base64url(&buffer)
}

/// RFC 7636 S256 challenge.
pub fn code_challenge(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

impl PkceLogin {
    /// Binds a loopback port chosen by the system and prepares the URL the
    /// browser opens.
    ///
    /// # Errors
    ///
    /// Returns [`OAuthError::Network`] when no loopback port can be bound.
    ///
    /// # Panics
    ///
    /// Never: the authorize URL is a valid constant.
    pub fn new(client_id: &str) -> Result<Self, OAuthError> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|error| OAuthError::Network(error.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| OAuthError::Network(error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| OAuthError::Network(error.to_string()))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/login");
        let state = random_token(16);
        let verifier = random_token(48);
        let mut url = Url::parse(AUTHORIZE_URL).expect("constant URL");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("state", &state)
            .append_pair("code_challenge", &code_challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", &SCOPES.join(" "));
        Ok(Self {
            listener,
            redirect_uri,
            state,
            verifier,
            auth_url: url.into(),
        })
    }

    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }

    /// Waits for the browser redirect. Unrelated requests (a favicon, a
    /// wrong state) are answered and ignored; the wait ends with a code,
    /// a denial, the cancel flag or the deadline.
    ///
    /// # Errors
    ///
    /// See [`OAuthError`].
    pub fn wait_for_code(
        &self,
        cancel: &AtomicBool,
        deadline: Instant,
        page: &CallbackPage,
    ) -> Result<String, OAuthError> {
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(OAuthError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(OAuthError::TimedOut);
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = self.handle_connection(stream, page) {
                        return result;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(OAuthError::Network(error.to_string())),
            }
        }
    }

    fn handle_connection(
        &self,
        mut stream: TcpStream,
        page: &CallbackPage,
    ) -> Option<Result<String, OAuthError>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buffer = [0_u8; 4096];
        let read = stream.read(&mut buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let result = parse_callback(request.lines().next().unwrap_or(""), &self.state);
        let body = match &result {
            Some(Ok(_)) => &page.success,
            Some(Err(_)) => &page.failure,
            None => &page.ignored,
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        result
    }

    /// Exchanges the code for an access token (PKCE, no client secret).
    ///
    /// # Errors
    ///
    /// Returns [`OAuthError::Network`] with a short reason; never the token.
    pub fn exchange(&self, client_id: &str, code: &str) -> Result<String, OAuthError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| OAuthError::Network(error.to_string()))?;
        let response = client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("client_id", client_id),
                ("code_verifier", self.verifier.as_str()),
            ])
            .send()
            .map_err(|error| OAuthError::Network(error.without_url().to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(OAuthError::Network(format!("token endpoint {status}")));
        }
        let body = response
            .bytes()
            .map_err(|_| OAuthError::Network("token endpoint answer".into()))?;
        let json: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| OAuthError::Network("token endpoint answer".into()))?;
        json.get("access_token")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| OAuthError::Network("token endpoint answer".into()))
    }
}

/// Localized HTML answered to the browser.
#[derive(Clone, Debug, Default)]
pub struct CallbackPage {
    pub success: String,
    pub failure: String,
    pub ignored: String,
}

impl CallbackPage {
    /// A minimal accessible page with one heading.
    pub fn from_texts(language: &str, success: &str, failure: &str) -> Self {
        let page = |text: &str| {
            let text = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!(
                "<!doctype html><html lang=\"{language}\"><head><meta charset=\"utf-8\"><title>ApricotPlayer</title></head><body><h1>{text}</h1></body></html>"
            )
        };
        Self {
            success: page(success),
            failure: page(failure),
            ignored: String::new(),
        }
    }
}

/// Parses the request line of a redirect. `None` means "not our callback"
/// (another path or a different state), which the listener ignores.
pub fn parse_callback(
    request_line: &str,
    expected_state: &str,
) -> Option<Result<String, OAuthError>> {
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return None;
    }
    let target = parts.next()?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/login" {
        return None;
    }
    let value = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if value("state").as_deref() != Some(expected_state) {
        return None;
    }
    if let Some(error) = value("error") {
        return Some(Err(if error == "access_denied" {
            OAuthError::Denied
        } else {
            OAuthError::BadCallback
        }));
    }
    Some(
        value("code")
            .filter(|code| !code.is_empty())
            .ok_or(OAuthError::BadCallback),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_is_unpadded_base64url_sha256() {
        // Reference value from Python hashlib + base64.urlsafe_b64encode.
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mJ92K1IrXQnKQd5AG3G8W0dGvgFOrA"),
            "pMOYl6cbZvX0WMoDvsPmOwXneDL-tIA4_G4PHydd9IA"
        );
    }

    #[test]
    fn callback_accepts_only_the_expected_state_and_path() {
        assert_eq!(
            parse_callback("GET /login?code=abc&state=s1 HTTP/1.1", "s1"),
            Some(Ok("abc".into()))
        );
        assert_eq!(
            parse_callback("GET /login?code=abc&state=other HTTP/1.1", "s1"),
            None
        );
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "s1"), None);
        assert_eq!(
            parse_callback("POST /login?code=abc&state=s1 HTTP/1.1", "s1"),
            None
        );
        assert_eq!(
            parse_callback("GET /login?error=access_denied&state=s1 HTTP/1.1", "s1"),
            Some(Err(OAuthError::Denied))
        );
        assert_eq!(
            parse_callback("GET /login?state=s1 HTTP/1.1", "s1"),
            Some(Err(OAuthError::BadCallback))
        );
    }

    #[test]
    fn auth_url_carries_pkce_state_and_loopback_redirect() {
        let login = PkceLogin::new("client").unwrap();
        let url = Url::parse(login.auth_url()).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "client");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["code_challenge"], code_challenge(&login.verifier));
        assert_eq!(query["state"], login.state);
        assert!(query["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(query["scope"].contains("streaming"));
    }

    #[test]
    fn wait_ends_on_cancel_deadline_and_real_callback() {
        let page = CallbackPage::from_texts("en", "ok", "failed");
        let login = PkceLogin::new("client").unwrap();
        let cancel = AtomicBool::new(true);
        let far = Instant::now() + Duration::from_secs(30);
        assert_eq!(
            login.wait_for_code(&cancel, far, &page),
            Err(OAuthError::Cancelled)
        );
        cancel.store(false, Ordering::SeqCst);
        assert_eq!(
            login.wait_for_code(&cancel, Instant::now(), &page),
            Err(OAuthError::TimedOut)
        );
        let port = login.listener.local_addr().unwrap().port();
        let state = login.state.clone();
        let browser = std::thread::spawn(move || {
            let mut stray = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stray
                .write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n")
                .unwrap();
            let mut answer = String::new();
            let _ = stray.read_to_string(&mut answer);
            let mut real = TcpStream::connect(("127.0.0.1", port)).unwrap();
            real.write_all(
                format!("GET /login?code=xyz&state={state} HTTP/1.1\r\n\r\n").as_bytes(),
            )
            .unwrap();
            let mut answer = String::new();
            let _ = real.read_to_string(&mut answer);
            answer
        });
        let code = login.wait_for_code(&cancel, far, &page);
        let answer = browser.join().unwrap();
        assert_eq!(code, Ok("xyz".into()));
        assert!(answer.contains("<h1>ok</h1>"));
    }
}

#[cfg(test)]
mod live_tests {
    #[test]
    #[ignore = "network"]
    fn exchange_with_a_bogus_code_reports_the_endpoint_status() {
        let login = super::PkceLogin::new("65b708073fc0480ea92a077233ca87bd").unwrap();
        let result = login.exchange("65b708073fc0480ea92a077233ca87bd", "bogus");
        eprintln!("exchange result: {result:?}");
    }
}
