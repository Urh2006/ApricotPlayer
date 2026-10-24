//! Read-only official history probe. Never starts a player or sends listens.
//! `baseline <app_data> <snapshot.json>`
//! `after <app_data> <snapshot.json> <private-target.json>` (target has {"uri":...})
//! Output contains only verdicts/counts; snapshots contain hashed identities.
use apricot_spotify::accounts::{SpotifyAccounts, unprotect_credentials};
use librespot_core::{Session, SessionConfig, authentication::Credentials};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize, Serialize)]
struct Snapshot {
    account_hash: String,
    captured_ms: u64,
    entries: Vec<Entry>,
}
#[derive(Deserialize, Serialize, Eq, Hash, PartialEq)]
struct Entry {
    track_hash: String,
    played_at: String,
}
fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn receipt_summary(before: &[Entry], after: &[Entry], target_hash: &str) -> (usize, bool) {
    let old: HashSet<_> = before.iter().collect();
    let new: Vec<_> = after.iter().filter(|entry| !old.contains(entry)).collect();
    (
        new.len(),
        new.iter().any(|entry| entry.track_hash == target_hash),
    )
}
fn entries(data: &serde_json::Value) -> Result<Vec<Entry>, &'static str> {
    data.get("items")
        .and_then(serde_json::Value::as_array)
        .ok_or("history_shape")?
        .iter()
        .map(|item| {
            let uri = item
                .pointer("/track/uri")
                .and_then(serde_json::Value::as_str)
                .filter(|uri| uri.starts_with("spotify:track:"))
                .ok_or("track_shape")?;
            let played_at = item
                .get("played_at")
                .and_then(serde_json::Value::as_str)
                .filter(|stamp| stamp.contains('T') && stamp.ends_with('Z'))
                .ok_or("timestamp_shape")?;
            Ok(Entry {
                track_hash: digest(uri),
                played_at: played_at.to_owned(),
            })
        })
        .collect()
}

#[allow(clippy::too_many_lines)] // Sequential, bounded developer probe; no app integration.
async fn run() -> Result<(), &'static str> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(args.len() == 3 && args[0] == "baseline" || args.len() == 4 && args[0] == "after") {
        return Err("usage");
    }
    let before = if args[0] == "after" {
        Some(
            serde_json::from_slice::<Snapshot>(
                &std::fs::read(&args[2]).map_err(|_| "snapshot_read")?,
            )
            .map_err(|_| "snapshot_shape")?,
        )
    } else {
        None
    };
    let target_hash = if args[0] == "after" {
        let target: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&args[3]).map_err(|_| "target_read")?)
                .map_err(|_| "target_shape")?;
        Some(digest(
            target
                .get("uri")
                .and_then(serde_json::Value::as_str)
                .filter(|uri| uri.starts_with("spotify:track:"))
                .ok_or("target_shape")?,
        ))
    } else {
        None
    };
    // Read directly: AccountStore::load can rename a malformed real account file.
    let accounts: SpotifyAccounts = serde_json::from_slice(
        &std::fs::read(Path::new(&args[1]).join("spotify_accounts.json"))
            .map_err(|_| "account_read")?,
    )
    .map_err(|_| "account_shape")?;
    let active = accounts.active.as_ref().ok_or("no_active_account")?;
    let account = accounts
        .accounts
        .iter()
        .find(|a| &a.key == active)
        .ok_or("active_account_missing")?;
    let blob = unprotect_credentials(&account.credentials).ok_or("credential_read")?;
    let credentials: Credentials = serde_json::from_slice(&blob).map_err(|_| "credential_shape")?;
    let session = Session::new(SessionConfig::default(), None);
    session
        .connect(credentials, false)
        .await
        .map_err(|_| "session_connect")?;
    let account_hash = digest(&session.username());
    if before
        .as_ref()
        .is_some_and(|snapshot| snapshot.account_hash != account_hash)
    {
        session.shutdown();
        return Err("account_changed");
    }
    let captured_ms = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "clock")?
            .as_millis(),
    )
    .map_err(|_| "clock")?;
    let result = async {
        let token = session
            .login5()
            .auth_token()
            .await
            .map_err(|_| "login5_token")?;
        // login5 may omit scope metadata: that is unknown, not permission denied.
        let scope = if token.scopes.is_empty() {
            "unknown"
        } else if token
            .scopes
            .iter()
            .any(|scope| scope == "user-read-recently-played")
        {
            "listed"
        } else {
            "not_listed"
        };
        println!("required_scope_metadata={scope}");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "http_client")?;
        let mut request = client
            .get("https://api.spotify.com/v1/me/player/recently-played")
            .bearer_auth(&token.access_token)
            .query(&[("limit", "50".to_owned())]);
        if let Some(snapshot) = &before {
            request = request.query(&[("after", snapshot.captured_ms)]);
        }
        let response = request.send().await.map_err(|_| "history_network")?;
        println!("history_http_status={}", response.status().as_u16());
        if response.status().as_u16() == 429 {
            if let Some(seconds) = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
            {
                println!("retry_after_seconds={seconds}");
            }
            return Err("history_rate_limited");
        }
        if !response.status().is_success() {
            return Err("history_unavailable");
        }
        let bytes = response.bytes().await.map_err(|_| "history_network")?;
        let data: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "history_json")?;
        let current = entries(&data)?;
        let more = data.get("next").is_some_and(|next| !next.is_null());
        if let Some(snapshot) = before {
            let (new_count, matched) = receipt_summary(
                &snapshot.entries,
                &current,
                target_hash.as_deref().ok_or("target_shape")?,
            );
            println!("new_entries={new_count} target_new_receipt={matched} more_pages={more}");
            println!(
                "verdict={}",
                if matched {
                    "target_history_ingestion_confirmed"
                } else {
                    "not_confirmed"
                }
            );
        } else {
            let snapshot = Snapshot {
                account_hash,
                captured_ms,
                entries: current,
            };
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&args[2])
                .map_err(|_| "snapshot_create")?;
            serde_json::to_writer(file, &snapshot).map_err(|_| "snapshot_write")?;
            println!(
                "entries={} more_pages={more} verdict=baseline_saved",
                snapshot.entries.len()
            );
        }
        Ok(())
    }
    .await;
    session.shutdown();
    result
}

#[tokio::main]
async fn main() {
    match tokio::time::timeout(Duration::from_secs(60), run()).await {
        Ok(Ok(())) => {}
        Ok(Err(stage)) => {
            println!("verdict=unverified stage={stage}");
            std::process::exit(2);
        }
        Err(_) => {
            println!("verdict=unverified stage=timeout");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_receipt_or_other_track_is_not_ingestion_proof() {
        let entry = |uri, time: &str| Entry {
            track_hash: digest(uri),
            played_at: time.into(),
        };
        let before = vec![entry("spotify:track:qa", "2026-10-14T15:00:00.000Z")];
        let old = vec![entry("spotify:track:qa", "2026-10-14T15:00:00.000Z")];
        assert_eq!(
            receipt_summary(&before, &old, &digest("spotify:track:qa")),
            (0, false)
        );
        let new = vec![entry("spotify:track:qa", "2026-10-14T15:05:00.000Z")];
        assert_eq!(
            receipt_summary(&before, &new, &digest("spotify:track:qa")),
            (1, true)
        );
        assert_eq!(
            receipt_summary(&before, &new, &digest("spotify:track:other")),
            (1, false)
        );
    }
    #[test]
    fn timestamped_track_receipt_is_required() {
        let data = serde_json::json!({"items":[{"track":{"uri":"spotify:track:qa"},"played_at":"2026-10-14T15:00:00.000Z"}]});
        let parsed = entries(&data).unwrap();
        assert_eq!(parsed[0].track_hash, digest("spotify:track:qa"));
        assert!(entries(&serde_json::json!({"playContexts":[]})).is_err());
        assert!(
            entries(&serde_json::json!({"items":[{"track":{"uri":"spotify:track:qa"}}]})).is_err()
        );
    }
}
