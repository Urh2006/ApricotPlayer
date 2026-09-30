//! Connect/queue spike: runs a silent LibreSpot Connect device named
//! "ApricotPlayer P0", prints every confirmed cluster update (context, current
//! track, prev/next tracks with provider and uid, queue revision, options) and
//! executes commands appended to `commands.txt` in the P0 data dir:
//!
//! * `add <uri>`            add_to_queue through the connect-state command API
//! * `remove <n>`           set_queue without the n-th queued ("queue" provider) item
//! * `move <n> <m>`         set_queue with the n-th queued item moved to index m
//! * `clear`                set_queue without any "queue" provider items
//! * `next` / `prev` / `pause` / `play`
//! * `shuffle on|off`, `repeat off|context|track`
//! * `load <context uri> [uid]`   Spirc::load of a context at an exact occurrence
//!
//! Queue edits go to this device through the same player command endpoint the
//! official clients use, so the result is whatever LibreSpot confirms in the
//! next cluster update; nothing here keeps a local copy of the queue.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use librespot_connect::{ConnectConfig, LoadRequest, LoadRequestOptions, PlayingTrack, Spirc};
use librespot_core::{config::SessionConfig, session::Session};
use librespot_playback::{
    audio_backend::Sink,
    config::PlayerConfig,
    mixer::{Mixer, MixerConfig, softmixer::SoftMixer},
    player::Player,
};
use librespot_protocol::connect::ClusterUpdate;
use librespot_protocol::player::ProvidedTrack;
use serde_json::{Value, json};

fn short(uri: &str) -> String {
    uri.rsplit(':')
        .next()
        .unwrap_or(uri)
        .chars()
        .take(6)
        .collect()
}

fn describe(update: &ClusterUpdate, own_device: &str) -> String {
    let cluster = &update.cluster;
    let state = &cluster.player_state;
    let opts = &state.options;
    let next: Vec<String> = state
        .next_tracks
        .iter()
        .take(10)
        .map(|t| {
            format!(
                "{}:{}:{}",
                t.provider,
                short(&t.uri),
                t.uid.chars().take(8).collect::<String>()
            )
        })
        .collect();
    format!(
        "reason={:?} active_is_me={} devices={} ctx={} track={}({}) prev={} next={} queue_rev={} shuffle={} repeat_ctx={} repeat_track={} paused={} pos={}\n        next: {}",
        update.update_reason,
        cluster.active_device_id == own_device,
        cluster.device.len(),
        short(&state.context_uri),
        short(&state.track.uri),
        state.track.provider,
        state.prev_tracks.len(),
        state.next_tracks.len(),
        state.queue_revision.chars().take(10).collect::<String>(),
        opts.shuffling_context,
        opts.repeating_context,
        opts.repeating_track,
        state.is_paused,
        state.position_as_of_timestamp,
        next.join(" ")
    )
}

fn track_json(track: &ProvidedTrack) -> Value {
    json!({
        "uri": track.uri, "uid": track.uid, "provider": track.provider,
        "metadata": track.metadata, "album_uri": track.album_uri, "artist_uri": track.artist_uri,
    })
}

pub async fn run() -> Result<()> {
    let cache = crate::cache()?;
    let credentials = cache
        .credentials()
        .ok_or_else(|| anyhow!("no stored credentials"))?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    let mixer: Arc<dyn Mixer> =
        Arc::new(SoftMixer::open(MixerConfig::default()).map_err(|e| anyhow!("mixer: {e}"))?);
    let player = Player::new(
        PlayerConfig::default(),
        session.clone(),
        mixer.get_soft_volume(),
        || Box::new(crate::audio::SilentSink::default()) as Box<dyn Sink>,
    );
    let latest: Arc<Mutex<Option<ClusterUpdate>>> = Arc::default();
    let mut cluster_stream = session
        .dealer()
        .listen_for(
            "hm://connect-state/v1/cluster",
            librespot_core::dealer::protocol::Message::from_raw::<ClusterUpdate>,
        )
        .map_err(|e| anyhow!("listen: {e}"))?;
    let name = std::env::var("SPOTIFY_P0_NAME").unwrap_or_else(|_| "ApricotPlayer P0".into());
    let config = ConnectConfig {
        name: name.clone(),
        ..Default::default()
    };
    let (spirc, task) = Spirc::new(config, session.clone(), credentials, player, mixer)
        .await
        .map_err(|e| anyhow!("spirc: {e}"))?;
    tokio::spawn(task);
    let device = session.device_id().to_string();
    println!("Connect device \"{name}\" is running (silent output).");

    {
        let latest = latest.clone();
        let device = device.clone();
        tokio::spawn(async move {
            while let Some(update) = cluster_stream.next().await {
                if let Ok(update) = update {
                    println!("CLUSTER {}", describe(&update, &device));
                    *latest.lock().unwrap() = Some(update);
                }
            }
        });
    }

    let file = if name == "ApricotPlayer P0" {
        "commands.txt".to_string()
    } else {
        format!("commands-{}.txt", name.len())
    };
    let commands = crate::data_dir().join(file);
    std::fs::write(&commands, "")?;
    let mut done = 0usize;
    let http = reqwest::Client::new();
    loop {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let text = std::fs::read_to_string(&commands).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        for line in lines.iter().skip(done) {
            done += 1;
            let parts: Vec<&str> = line.split_whitespace().collect();
            println!("COMMAND {line}");
            let result: Result<()> = async {
                match parts.as_slice() {
                    ["quit"] => {
                        spirc.shutdown().map_err(|e| anyhow!("{e}"))?;
                        std::process::exit(0);
                    }
                    ["next"] => spirc.next().map_err(|e| anyhow!("{e}")),
                    ["prev"] => spirc.prev().map_err(|e| anyhow!("{e}")),
                    ["pause"] => spirc.pause().map_err(|e| anyhow!("{e}")),
                    ["play"] => spirc.play().map_err(|e| anyhow!("{e}")),
                    ["activate"] => spirc.activate().map_err(|e| anyhow!("{e}")),
                    ["shuffle", v] => spirc.shuffle(*v == "on").map_err(|e| anyhow!("{e}")),
                    ["repeat", "off"] => {
                        spirc.repeat(false).map_err(|e| anyhow!("{e}"))?;
                        spirc.repeat_track(false).map_err(|e| anyhow!("{e}"))
                    }
                    ["repeat", "context"] => spirc.repeat(true).map_err(|e| anyhow!("{e}")),
                    ["repeat", "track"] => spirc.repeat_track(true).map_err(|e| anyhow!("{e}")),
                    ["load", context, rest @ ..] => {
                        let options = LoadRequestOptions {
                            start_playing: true,
                            playing_track: rest.first().map(|uid| PlayingTrack::Uid((*uid).to_string())),
                            ..Default::default()
                        };
                        spirc.load(LoadRequest::from_context_uri((*context).to_string(), options)).map_err(|e| anyhow!("{e}"))
                    }
                    ["add", uri] => {
                        send_command(&http, &session, &device, json!({
                            "endpoint": "add_to_queue",
                            "track": { "uri": uri, "metadata": { "is_queued": "true" }, "provider": "queue" },
                            "logging_params": {}
                        }))
                        .await
                    }
                    [op @ ("remove" | "move" | "clear"), rest @ ..] => {
                        let update = latest.lock().unwrap().clone().ok_or_else(|| anyhow!("no cluster yet"))?;
                        let state = &update.cluster.player_state;
                        let mut next: Vec<ProvidedTrack> = state.next_tracks.clone();
                        let queued: Vec<usize> =
                            next.iter().enumerate().filter(|(_, t)| t.provider == "queue").map(|(i, _)| i).collect();
                        match *op {
                            "clear" => next.retain(|t| t.provider != "queue"),
                            "remove" => {
                                let n: usize = rest.first().ok_or_else(|| anyhow!("remove <n>"))?.parse()?;
                                let index = *queued.get(n).ok_or_else(|| anyhow!("no queued item {n}"))?;
                                next.remove(index);
                            }
                            _ => {
                                let n: usize = rest.first().ok_or_else(|| anyhow!("move <n> <m>"))?.parse()?;
                                let m: usize = rest.get(1).ok_or_else(|| anyhow!("move <n> <m>"))?.parse()?;
                                let index = *queued.get(n).ok_or_else(|| anyhow!("no queued item {n}"))?;
                                let track = next.remove(index);
                                let target = *queued.get(m).unwrap_or(&index);
                                next.insert(target.min(next.len()), track);
                            }
                        }
                        let target = if update.cluster.active_device_id.is_empty() {
                            device.clone()
                        } else {
                            update.cluster.active_device_id.clone()
                        };
                        send_command_to(&http, &session, &device, &target, json!({
                            "endpoint": "set_queue",
                            "next_tracks": next.iter().map(track_json).collect::<Vec<_>>(),
                            "prev_tracks": state.prev_tracks.iter().map(track_json).collect::<Vec<_>>(),
                            "queue_revision": state.queue_revision,
                            "logging_params": {}
                        }))
                        .await
                    }
                    _ => Err(anyhow!("unknown command")),
                }
            }
            .await;
            if let Err(error) = result {
                println!("COMMAND FAILED {error}");
            }
        }
    }
}

async fn send_command(
    http: &reqwest::Client,
    session: &Session,
    device: &str,
    command: Value,
) -> Result<()> {
    send_command_to(http, session, device, device, command).await
}

/// Sends a player command from this device to `target` (own device or the
/// currently active remote device), exactly like the official clients.
async fn send_command_to(
    http: &reqwest::Client,
    session: &Session,
    device: &str,
    target: &str,
    command: Value,
) -> Result<()> {
    let token = session
        .login5()
        .auth_token()
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let client_token = session
        .spclient()
        .client_token()
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let base = session
        .spclient()
        .base_url()
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let url = format!("{base}/connect-state/v1/player/command/from/{device}/to/{target}");
    let response = http
        .post(url)
        .bearer_auth(token.access_token)
        .header("client-token", client_token)
        .json(&json!({ "command": command }))
        .send()
        .await?;
    println!("COMMAND status {}", response.status());
    Ok(())
}
