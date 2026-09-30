//! Spotify P0 feasibility spike for ApricotPlayer 2.0.
//!
//! Evidence tool only. It never prints or stores access tokens, usernames,
//! e-mail addresses or playlist/track names in its evidence output. Reusable
//! LibreSpot credentials are kept in a scratch directory outside the repository
//! (`SPOTIFY_P0_DIR`, default `%LOCALAPPDATA%\ApricotPlayer-spotify-p0`) and
//! must be deleted at the end of P0.

mod audio;
mod bridge;
mod connect;
mod pb;
mod probe;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use librespot_core::{
    authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
};

/// Scopes requested by the upstream `librespot --enable-oauth` binary (v0.8.0 src/main.rs).
pub const OAUTH_SCOPES: &[&str] = &[
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

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SPOTIFY_P0_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("ApricotPlayer-spotify-p0")
}

pub fn cache() -> Result<Cache> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    Cache::new(
        Some(dir.join("credentials")),
        Some(dir.join("volume")),
        Some(dir.join("audio")),
        Some(512 * 1024 * 1024),
    )
    .map_err(|e| anyhow!("cache: {e}"))
}

pub async fn connect_cached() -> Result<Session> {
    let cache = cache()?;
    let credentials = cache
        .credentials()
        .ok_or_else(|| anyhow!("no stored credentials; run `login` or `discover` first"))?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    let started = Instant::now();
    session
        .connect(credentials, true)
        .await
        .map_err(|e| anyhow!("connect: {e}"))?;
    eprintln!("connected in {} ms", started.elapsed().as_millis());
    Ok(session)
}

fn print_account(session: &Session) {
    // Account type and market only, no personal identifiers.
    println!(
        "account type={} country={} catalogue={} filter-explicit={} autoplay-attr={}",
        session
            .get_user_attribute("type")
            .unwrap_or_else(|| "?".into()),
        session.country(),
        session
            .get_user_attribute("catalogue")
            .unwrap_or_else(|| "?".into()),
        session.filter_explicit_content(),
        session.autoplay(),
    );
}

async fn login(port: u16) -> Result<()> {
    let config = SessionConfig::default();
    let redirect = format!("http://127.0.0.1:{port}/login");
    let started = Instant::now();
    let client = librespot_oauth::OAuthClientBuilder::new(
        &config.client_id,
        &redirect,
        OAUTH_SCOPES.to_vec(),
    )
    .with_custom_message("ApricotPlayer P0: prijava je uspela, to okno lahko zaprete.")
    .build()
    .map_err(|e| anyhow!("oauth client: {e}"))?;
    let token = client
        .get_access_token_async()
        .await
        .map_err(|e| anyhow!("oauth: {e}"))?;
    println!(
        "oauth ok in {} ms, scopes granted={}, refresh token present={}",
        started.elapsed().as_millis(),
        token.scopes.len(),
        !token.refresh_token.is_empty()
    );
    let session = Session::new(config, Some(cache()?));
    session
        .connect(Credentials::with_access_token(token.access_token), true)
        .await
        .map_err(|e| anyhow!("connect: {e}"))?;
    println!("session ok, reusable credentials stored in scratch dir");
    print_account(&session);
    Ok(())
}

async fn discover() -> Result<()> {
    let config = SessionConfig::default();
    let mut discovery =
        librespot_discovery::Discovery::builder(config.device_id.clone(), config.client_id.clone())
            .name("ApricotPlayer P0")
            .launch()
            .map_err(|e| anyhow!("discovery: {e}"))?;
    println!("Zeroconf device \"ApricotPlayer P0\" is visible; pick it in a Spotify app.");
    let credentials = discovery
        .next()
        .await
        .ok_or_else(|| anyhow!("discovery ended"))?;
    let session = Session::new(config, Some(cache()?));
    session
        .connect(credentials, true)
        .await
        .map_err(|e| anyhow!("connect: {e}"))?;
    println!("discovery login ok, reusable credentials stored in scratch dir");
    print_account(&session);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "login" => {
            let port = args.get(1).map(|p| p.parse()).transpose()?.unwrap_or(5588);
            login(port).await
        }
        "discover" => discover().await,
        "whoami" => {
            let session = connect_cached().await?;
            print_account(&session);
            Ok(())
        }
        "probe" => {
            let out = args.get(1).cloned().unwrap_or_else(|| "probe.json".into());
            let session = connect_cached().await?;
            print_account(&session);
            probe::run(&session, &out).await
        }
        "play" => {
            let uri = args.get(1).context("play <spotify uri> [seconds]")?.clone();
            let secs = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(20u64);
            let session = connect_cached().await?;
            audio::play_capture(session, &uri, secs).await
        }
        "connect" => connect::run().await,
        "gapless" => {
            let first = args.get(1).context("gapless <uri1> <uri2>")?.clone();
            let second = args.get(2).context("gapless <uri1> <uri2>")?.clone();
            let session = connect_cached().await?;
            audio::gapless(session, &first, &second).await
        }
        "bridge" => {
            let uri = args
                .get(1)
                .context("bridge <spotify uri> [libmpv dll]")?
                .clone();
            let dll = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| r"C:\Program Files\ApricotPlayer\mpv\libmpv-2.dll".into());
            let session = connect_cached().await?;
            bridge::run(session, &uri, &dll).await
        }
        "raw" => {
            let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
            let session = connect_cached().await?;
            probe::raw(&session, &arg(1), &arg(2), &arg(3), &arg(4)).await
        }
        "cwrite" | "cremove" => {
            let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
            let session = connect_cached().await?;
            probe::collection_write(&session, &arg(1), &arg(2), &arg(3), cmd == "cremove").await
        }
        "citems" => {
            let session = connect_cached().await?;
            probe::collection_items(&session, args.get(1).map(String::as_str).unwrap_or("ban"))
                .await
        }
        "forget" => {
            let dir = data_dir();
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            println!("removed scratch credentials and cache");
            Ok(())
        }
        _ => {
            eprintln!(
                "usage: spotify-p0 login [port] | discover | whoami | probe [out.json] | play <uri> [secs] | forget"
            );
            Ok(())
        }
    }
}
