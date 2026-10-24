//! Bounded subtitle HTTP transport; invoked only from a background worker.

use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, COOKIE, HOST, HeaderMap, HeaderName, HeaderValue,
    LOCATION, PROXY_AUTHORIZATION, REFERER, USER_AGENT,
};
use serde_json::Value;
use std::{io::Read, time::Duration};
use url::Url;

const MAX_BYTES: u64 = 5_000_000;
/// Python `transcript_request_headers` fallback when no cookie user agent is set.
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("invalid transcript URL or request headers")]
    InvalidRequest,
    #[error("transcript service rate limited the request")]
    RateLimited,
    #[error("transcript request failed")]
    Request,
    /// Worded like Python's `HTTPError`, for example "HTTP Error 403: Forbidden".
    #[error("HTTP Error {0}")]
    Http(String),
    #[error("transcript exceeded the response size limit")]
    TooLarge,
}

fn remote_url(value: &str) -> Result<Url, TranscriptError> {
    let url = Url::parse(value).map_err(|_| TranscriptError::InvalidRequest)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(TranscriptError::InvalidRequest);
    }
    Ok(url)
}

fn request_headers(
    info: &Value,
    track: &Value,
    source: &str,
    agent: &str,
) -> Result<HeaderMap, TranscriptError> {
    let mut headers = HeaderMap::new();
    for candidate in [info, track] {
        if let Some(values) = candidate.get("http_headers").and_then(Value::as_object) {
            for (name, value) in values {
                let Some(value) = value.as_str().filter(|value| !value.is_empty()) else {
                    continue;
                };
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| TranscriptError::InvalidRequest)?;
                let value =
                    HeaderValue::from_str(value).map_err(|_| TranscriptError::InvalidRequest)?;
                headers.insert(name, value);
            }
        }
    }
    // These are transport-owned, not extractor-controlled.
    headers.remove(HOST);
    headers.remove(PROXY_AUTHORIZATION);
    for (name, value) in [
        (
            USER_AGENT,
            if agent.trim().is_empty() {
                DEFAULT_USER_AGENT
            } else {
                agent.trim()
            },
        ),
        (ACCEPT_LANGUAGE, "en-US,en;q=0.9"),
        (REFERER, source),
    ] {
        if !value.is_empty() && !headers.contains_key(&name) {
            headers.insert(
                name,
                HeaderValue::from_str(value).map_err(|_| TranscriptError::InvalidRequest)?,
            );
        }
    }
    headers.insert(ACCEPT, HeaderValue::from_static("text/vtt,text/plain,*/*"));
    Ok(headers)
}

/// Downloads selected subtitle text using extractor-provided headers.
///
/// # Errors
/// Returns bounded transport, invalid request, size, or rate-limit errors.
pub fn fetch_text(
    track: &Value,
    info: &Value,
    source: &str,
    agent: &str,
    proxy: Option<&str>,
) -> Result<String, TranscriptError> {
    let mut url = remote_url(
        track
            .get("url")
            .and_then(Value::as_str)
            .ok_or(TranscriptError::InvalidRequest)?,
    )?;
    let mut headers = request_headers(info, track, source, agent)?;
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if let Some(proxy) = proxy.map(str::trim).filter(|proxy| !proxy.is_empty()) {
        builder =
            builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| TranscriptError::InvalidRequest)?);
    }
    let client = builder.build().map_err(|_| TranscriptError::Request)?;
    for _ in 0..=5 {
        let response = client
            .get(url.clone())
            .headers(headers.clone())
            .send()
            .map_err(|_| TranscriptError::Request)?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(TranscriptError::RateLimited);
        }
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(TranscriptError::Request)?;
            let next = url
                .join(location)
                .map_err(|_| TranscriptError::InvalidRequest)?;
            let next = remote_url(next.as_str())?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err(TranscriptError::InvalidRequest);
            }
            if url.origin() != next.origin() {
                headers.remove(AUTHORIZATION);
                headers.remove(COOKIE);
                headers.remove(REFERER);
            }
            url = next;
            continue;
        }
        if !response.status().is_success() {
            let status = response.status();
            return Err(TranscriptError::Http(format!(
                "{}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            )));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BYTES)
        {
            return Err(TranscriptError::TooLarge);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| TranscriptError::Request)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(TranscriptError::TooLarge);
        }
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    Err(TranscriptError::Request)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn track_headers_override_info_without_overriding_transport() {
        let info = serde_json::json!({"http_headers":{"User-Agent":"Info", "Accept":"wrong"}});
        let track = serde_json::json!({"http_headers":{"User-Agent":"Track", "Host":"wrong"}});
        let headers =
            request_headers(&info, &track, "https://example.test/watch", "Configured").unwrap();
        assert_eq!(headers[USER_AGENT], "Track");
        assert_eq!(headers[ACCEPT], "text/vtt,text/plain,*/*");
        assert!(!headers.contains_key(HOST));
        assert!(remote_url("file:///secret").is_err());
        assert!(remote_url("https://user:pass@example.test/").is_err());
    }

    #[test]
    fn local_http_success_rate_limit_and_size_errors() {
        use std::{io::Write, net::TcpListener, thread};
        for (status, length, body, expected) in [
            ("200 OK", 7, "WEBVTT\n", 0),
            ("429 Too Many Requests", 0, "", 1),
            ("200 OK", 5_000_001, "", 2),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                let _ = socket.read(&mut request);
                write!(socket, "HTTP/1.1 {status}\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{body}").unwrap();
            });
            let track = serde_json::json!({"url":format!("http://{address}/captions")});
            let result = fetch_text(&track, &Value::Null, "", "", None);
            server.join().unwrap();
            match expected {
                0 => assert_eq!(result.unwrap(), "WEBVTT\n"),
                1 => assert!(matches!(result, Err(TranscriptError::RateLimited))),
                _ => assert!(matches!(result, Err(TranscriptError::TooLarge))),
            }
        }
    }

    #[test]
    fn cross_origin_redirect_drops_credentials() {
        use std::{io::Write, net::TcpListener, thread};
        let first = TcpListener::bind("127.0.0.1:0").unwrap();
        let second = TcpListener::bind("127.0.0.1:0").unwrap();
        let start = first.local_addr().unwrap();
        let target = second.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = first.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 4096];
            let _ = socket.read(&mut buffer);
            write!(socket, "HTTP/1.1 302 Found\r\nLocation: http://{target}/final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            drop(socket);
            let (mut socket, _) = second.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let count = socket.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..count]).to_lowercase();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            request
        });
        let track = serde_json::json!({"url":format!("http://{start}/start"), "http_headers":{"Authorization":"Bearer test", "Cookie":"session=test"}});
        let result = fetch_text(
            &track,
            &Value::Null,
            "https://example.test/private",
            "",
            None,
        );
        let request = server.join().unwrap();
        assert_eq!(result.unwrap(), "");
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("cookie:"));
        assert!(!request.contains("referer:"));
    }
}
