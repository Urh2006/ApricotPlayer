// Developer probe for the internal Spotify interfaces, used while building
// the adapter. It uses the active saved account of an Apricot data folder
// (DPAPI, the same Windows user) and writes raw answers only to `out_dir`,
// which must be outside the repository. Tokens are never printed.
//
// cargo run -p apricot-spotify --example spotify_probe -- <app_data> <out_dir> hashes
// ... pf <operation> '<variables json>'
// ... sp <GET|POST> <path> ['<body json>']
// ... grep <text>   (prints web-player code around <text>, to learn variables)

#![allow(
    clippy::too_many_lines,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::format_push_string
)] // A developer tool, not part of the application.

use std::path::PathBuf;

use apricot_spotify::{AccountStore, api::Api};
use librespot_core::{Session, SessionConfig, authentication::Credentials};

fn summary(value: &serde_json::Value, depth: usize) -> String {
    match value {
        serde_json::Value::Object(map) if depth < 4 => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, value)| format!("{key}: {}", summary(value, depth + 1)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        serde_json::Value::Array(items) => format!(
            "[{} x {}]",
            items.len(),
            items
                .first()
                .map_or(String::new(), |item| summary(item, depth + 1))
        ),
        serde_json::Value::String(_) => "s".into(),
        _ if depth >= 4 => "…".into(),
        other => other.to_string(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let app_data = PathBuf::from(&arguments[0]);
    let out = PathBuf::from(&arguments[1]);
    std::fs::create_dir_all(&out)?;
    let api = Api::new(Some(out.join("hashes.json")));
    let command = arguments[2].as_str();
    if command == "hashes" {
        let hashes = api.refresh_hashes().await?;
        let mut names: Vec<_> = hashes.keys().cloned().collect();
        names.sort();
        std::fs::write(out.join("operations.txt"), names.join("\n"))?;
        println!("{} operations", names.len());
        return Ok(());
    }
    if command == "grep" {
        let http = reqwest::Client::builder()
            .user_agent(apricot_spotify::api::BROWSER_AGENT)
            .build()?;
        let html = http
            .get("https://open.spotify.com/")
            .send()
            .await?
            .text()
            .await?;
        let mut scripts: Vec<String> = html
            .split("src=\"")
            .skip(1)
            .filter_map(|part| part.split('"').next())
            .filter(|src| src.ends_with(".js") && src.contains("/web-player/"))
            .map(str::to_owned)
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut found = String::new();
        while let Some(script) = scripts.pop() {
            if !seen.insert(script.clone()) || seen.len() > 1500 {
                continue;
            }
            let Ok(text) = http.get(&script).send().await?.text().await else {
                continue;
            };
            for (at, _) in text.match_indices(arguments[3].as_str()) {
                let start = text.floor_char_boundary(at.saturating_sub(700));
                let end = text.ceil_char_boundary((at + 900).min(text.len()));
                found.push_str(&format!("\n==== {script}\n{}\n", &text[start..end]));
            }
            for chunk in apricot_spotify::api::chunk_files(&text) {
                if !seen.contains(&chunk) {
                    scripts.push(chunk);
                }
            }
            for chunk in text
                .split('"')
                .filter(|part| part.ends_with(".js") && part.len() < 120 && !part.contains(' '))
            {
                let url = if chunk.starts_with("http") {
                    chunk.to_owned()
                } else {
                    format!("https://open.spotifycdn.com/cdn/build/web-player/{chunk}")
                };
                if !seen.contains(&url) {
                    scripts.push(url);
                }
            }
        }
        let name: String = arguments[3]
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        std::fs::write(out.join(format!("grep-{name}.txt")), &found)?;
        println!("{} bytes of matches", found.len());
        return Ok(());
    }
    let store = AccountStore::new(&app_data);
    let accounts = store.load();
    let key = accounts.active.clone().ok_or("no active account")?;
    let account = accounts
        .accounts
        .iter()
        .find(|account| account.key == key)
        .ok_or("no account")?;
    let blob = apricot_spotify::accounts::unprotect_credentials(&account.credentials)
        .ok_or("credentials unreadable")?;
    let credentials: Credentials = serde_json::from_slice(&blob)?;
    let session = Session::new(SessionConfig::default(), None);
    session.connect(credentials, false).await?;
    let (name, answer) = match command {
        "pf" => (
            arguments[3].clone(),
            api.pathfinder(
                &session,
                &arguments[3],
                serde_json::from_str(&arguments[4])?,
            )
            .await?,
        ),
        "sp" => {
            let path = arguments[4].replace("{user}", &session.username());
            let method = reqwest::Method::from_bytes(arguments[3].as_bytes())?;
            let body = arguments
                .get(5)
                .map(|body| serde_json::from_str(&body.replace("{user}", &session.username())))
                .transpose()?;
            (
                format!("sp-{}", arguments[4]),
                api.spclient(&session, method, &path, body).await?,
            )
        }
        _ => return Err("unknown command".into()),
    };
    let file: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(80)
        .collect();
    std::fs::write(
        out.join(format!("{file}.json")),
        serde_json::to_vec_pretty(&answer)?,
    )?;
    println!("{}", summary(&answer, 0));
    Ok(())
}
