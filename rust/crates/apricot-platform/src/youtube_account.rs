//! The name of the YouTube account in the cookies file, for My YouTube
//! (2.2.0). YouTube's web page asks its account menu with the cookies and a
//! `SAPISIDHASH` authorization; that one read request is the only call
//! ApricotPlayer makes to YouTube outside yt-dlp, and it never changes the
//! account.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha1::{Digest, Sha1};

const ORIGIN: &str = "https://www.youtube.com";
const ACCOUNT_MENU_URL: &str =
    "https://www.youtube.com/youtubei/v1/account/account_menu?prettyPrint=false";
const WEB_CLIENT_VERSION: &str = "2.20251120.01.00";

/// The cookies of a Netscape `cookies.txt` that YouTube receives.
fn youtube_cookies(netscape: &str) -> Vec<(String, String)> {
    netscape
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [domain, _, _, _, _, name, value] = fields.as_slice() else {
                return None;
            };
            let domain = domain.trim_start_matches('.').to_ascii_lowercase();
            (domain == "youtube.com" || domain.ends_with(".youtube.com"))
                .then(|| ((*name).to_owned(), (*value).to_owned()))
        })
        .collect()
}

/// YouTube's web authorization: `SAPISIDHASH <time>_<sha1(time sapisid origin)>`.
fn sapisid_hash(sapisid: &str, seconds: u64) -> String {
    let digest = Sha1::digest(format!("{seconds} {sapisid} {ORIGIN}").as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("SAPISIDHASH {seconds}_{hex}")
}

/// The account name in an account menu answer.
fn account_name_of(answer: &Value) -> Option<String> {
    let header = answer.pointer(
        "/actions/0/openPopupAction/popup/multiPageMenuRenderer/header/activeAccountHeaderRenderer",
    )?;
    let name = header
        .pointer("/accountName/simpleText")
        .and_then(Value::as_str)
        .or_else(|| {
            header
                .pointer("/accountName/runs/0/text")
                .and_then(Value::as_str)
        })?
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The signed-in account's name, `None` when the cookies hold no sign-in
/// YouTube accepts.
///
/// # Errors
/// The network error text.
pub fn account_name(netscape: &str) -> Result<Option<String>, String> {
    let cookies = youtube_cookies(netscape);
    let Some(sapisid) = cookies
        .iter()
        .find(|(name, _)| name == "SAPISID")
        .or_else(|| cookies.iter().find(|(name, _)| name == "__Secure-3PAPISID"))
        .map(|(_, value)| value.clone())
    else {
        return Ok(None);
    };
    let header = cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB", "clientVersion": WEB_CLIENT_VERSION, "hl": "en" } }
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(ACCOUNT_MENU_URL)
        .header(reqwest::header::COOKIE, header)
        .header(reqwest::header::AUTHORIZATION, sapisid_hash(&sapisid, seconds))
        .header("X-Origin", ORIGIN)
        .header(reqwest::header::ORIGIN, ORIGIN)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36",
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .map_err(|error| error.to_string())?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("YouTube answered {}", response.status()));
    }
    let answer: Value = serde_json::from_str(&response.text().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    Ok(account_name_of(&answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_youtube_cookies_go_to_youtube() {
        let text = "# Netscape HTTP Cookie File\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tabc\n\
            #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tsid\n\
            .google.com\tTRUE\t/\tTRUE\t0\tSAPISID\tother\n\
            broken line\n";
        assert_eq!(
            youtube_cookies(text),
            [
                ("SAPISID".to_owned(), "abc".to_owned()),
                ("SID".to_owned(), "sid".to_owned())
            ]
        );
    }

    #[test]
    fn the_authorization_follows_youtubes_web_format() {
        // sha1("1700000000 abc https://www.youtube.com")
        let expected: String = Sha1::digest(b"1700000000 abc https://www.youtube.com")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(expected.len(), 40);
        assert_eq!(
            sapisid_hash("abc", 1_700_000_000),
            format!("SAPISIDHASH 1700000000_{expected}")
        );
    }

    #[test]
    fn the_account_menu_names_the_account() {
        let answer = serde_json::json!({ "actions": [{ "openPopupAction": { "popup": {
            "multiPageMenuRenderer": { "header": { "activeAccountHeaderRenderer": {
                "accountName": { "simpleText": "Urh" } } } } } } }] });
        assert_eq!(account_name_of(&answer).as_deref(), Some("Urh"));
        assert_eq!(account_name_of(&serde_json::json!({})), None);
    }

    #[test]
    fn cookies_without_a_sign_in_ask_nothing() {
        assert_eq!(account_name("# Netscape HTTP Cookie File\n"), Ok(None));
    }
}
