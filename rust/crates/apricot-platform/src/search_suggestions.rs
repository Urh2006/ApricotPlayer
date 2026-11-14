//! `YouTube` search suggestions for a typed query; called off the UI thread.

use std::time::Duration;

/// The suggestion service's answer for `query`, as text.
///
/// # Errors
///
/// Returns an error when the service cannot be reached.
pub fn youtube_suggestions(query: &str, language: &str, proxy: &str) -> Result<String, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent(format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")));
    if !proxy.trim().is_empty() {
        builder =
            builder.proxy(reqwest::Proxy::all(proxy.trim()).map_err(|error| error.to_string())?);
    }
    let client = builder.build().map_err(|error| error.to_string())?;
    let response = client
        .get("https://suggestqueries.google.com/complete/search")
        .query(&[
            ("client", "firefox"),
            ("ds", "yt"),
            ("hl", language),
            ("q", query.trim()),
        ])
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let bytes = response.bytes().map_err(|error| error.to_string())?;
    // The service answers in the language's own encoding when it is not
    // UTF-8; a lossy read keeps every suggestion that is readable.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
