//! Real-process qualification for mpv JSON IPC on Windows.

#[cfg(windows)]
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
#[cfg(windows)]
use apricot_playback::{
    MpvIpcClient, MpvLaunchOptions, MpvProcessEngine, PlaybackCommand, PlaybackEngine,
    PlaybackEvent, make_unique_ipc_path,
};
#[cfg(windows)]
use serde_json::{Value, json};

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..");
    let mpv = repository.join("vendor").join("mpv").join("mpv.exe");
    if !mpv.is_file() {
        return Err(format!("bundled mpv was not found at {}", mpv.display()).into());
    }
    let fixture = temporary_wav_path();
    write_silent_stereo_wav(&fixture, 44_100, 2)?;
    let pipe = make_unique_ipc_path();
    let mut process = start_mpv(&mpv, &fixture, &pipe)?;
    let client = MpvIpcClient::new(pipe);

    let result = qualify(&client);
    let _ = client.request(json!(["quit"]), Duration::from_secs(1));
    let _ = process.wait();
    if result.is_ok() {
        qualify_process_engine(&mpv, &fixture)?;
    }
    let _ = fs::remove_file(&fixture);
    result
}

#[cfg(windows)]
fn qualify_process_engine(mpv: &Path, fixture: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut options = MpvLaunchOptions::new(mpv);
    options.audio_driver = Some("null".to_owned());
    options.cache = None;
    options.initial_volume = 37.0;
    let spawn_started = Instant::now();
    let mut engine = MpvProcessEngine::spawn(&options)?;
    println!(
        "MPV_PROCESS_INITIALIZE_MS={:.3}",
        spawn_started.elapsed().as_secs_f64() * 1_000.0
    );
    let item = MediaItem {
        id: MediaId("qualification-fixture".to_owned()),
        source: MediaSource::Local,
        kind: MediaKind::Audio,
        title: "Qualification fixture".to_owned(),
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: Some(fixture.to_string_lossy().into_owned()),
        channel: String::new(),
        duration_seconds: Some(2.0),
        metadata: BTreeMap::default(),
    };
    let first_load_started = Instant::now();
    engine.execute(PlaybackCommand::Load {
        item: Box::new(item.clone()),
        start_position_seconds: None,
    })?;
    wait_for_engine_event(&mut engine, Duration::from_secs(3), |event| {
        matches!(event, PlaybackEvent::Started)
    })?;
    println!(
        "MPV_PROCESS_FIRST_LOAD_MS={:.3}",
        first_load_started.elapsed().as_secs_f64() * 1_000.0
    );
    engine.execute(PlaybackCommand::SetVolume(42.0))?;
    engine.execute(PlaybackCommand::SetSpeed(1.25))?;
    engine.execute(PlaybackCommand::SetPitch(1.1))?;
    engine.execute(PlaybackCommand::SeekAbsolute {
        seconds: 0.5,
        exact: true,
    })?;
    wait_for_engine_event(&mut engine, Duration::from_secs(3), |event| {
        matches!(
            event,
            PlaybackEvent::Position {
                duration: Some(duration),
                ..
            } if *duration >= 1.9
        )
    })?;

    let second_load_started = Instant::now();
    engine.execute(PlaybackCommand::Load {
        item: Box::new(item),
        start_position_seconds: None,
    })?;
    wait_for_engine_event(&mut engine, Duration::from_secs(3), |event| {
        matches!(event, PlaybackEvent::Started)
    })?;
    println!(
        "MPV_PROCESS_SECOND_LOAD_MS={:.3}",
        second_load_started.elapsed().as_secs_f64() * 1_000.0
    );
    engine.execute(PlaybackCommand::Stop)?;
    println!("MPV_PROCESS_ENGINE=PASS");
    Ok(())
}

#[cfg(windows)]
fn wait_for_engine_event(
    engine: &mut MpvProcessEngine,
    timeout: Duration,
    predicate: impl Fn(&PlaybackEvent) -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(event) = engine.poll_event()? {
            if let PlaybackEvent::Failed(error) = &event {
                return Err(error.clone().into());
            }
            if predicate(&event) {
                return Ok(());
            }
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Err("expected mpv engine event was not received".into())
}

#[cfg(windows)]
fn qualify(client: &MpvIpcClient) -> Result<(), Box<dyn std::error::Error>> {
    let version = data(&client.request(
        json!(["get_property", "mpv-version"]),
        Duration::from_secs(3),
    )?)?;
    println!("MPV_VERSION={version}");

    let duration = wait_for_property(client, "duration", Duration::from_secs(3))?;
    println!("DURATION={duration}");

    assert_success(&client.request(
        json!(["set_property", "volume", 37]),
        Duration::from_secs(1),
    )?)?;
    assert_success(&client.request(
        json!(["set_property", "speed", 1.25]),
        Duration::from_secs(1),
    )?)?;
    assert_success(&client.request(
        json!(["set_property", "pitch", 1.1]),
        Duration::from_secs(1),
    )?)?;
    assert_success(&client.request(
        json!(["seek", 0.1, "absolute+exact"]),
        Duration::from_secs(1),
    )?)?;

    let filter = "@apricot_eq:lavfi=[volume=-3.0dB,equalizer=f=31:t=q:w=1.2:g=3.0,equalizer=f=1000:t=q:w=1.8:g=-2.0,alimiter=limit=0.95:attack=5:release=50]";
    assert_success(&client.request(json!(["af", "add", filter]), Duration::from_secs(2))?)?;
    let filters = data(&client.request(json!(["get_property", "af"]), Duration::from_secs(1))?)?;
    println!("FILTERS={filters}");

    let devices = data(&client.request(
        json!(["get_property", "audio-device-list"]),
        Duration::from_secs(1),
    )?)?;
    let device_count = devices.as_array().map_or(0, Vec::len);
    println!("AUDIO_DEVICES={device_count}");
    println!("MPV_IPC_SPIKE=PASS");
    Ok(())
}

#[cfg(windows)]
fn wait_for_property(
    client: &MpvIpcClient,
    property: &str,
    timeout: Duration,
) -> Result<Value, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        let response = client.request(
            json!(["get_property", property]),
            Duration::from_millis(500),
        )?;
        if response.get("error").and_then(Value::as_str) == Some("success") {
            return data(&response);
        }
        if Instant::now() >= deadline {
            return Err(
                format!("mpv property '{property}' did not become available: {response}").into(),
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
fn data(response: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    assert_success(response)?;
    response
        .get("data")
        .cloned()
        .ok_or_else(|| "mpv response did not contain data".into())
}

#[cfg(windows)]
fn assert_success(response: &Value) -> Result<(), Box<dyn std::error::Error>> {
    if response.get("error").and_then(Value::as_str) == Some("success") {
        Ok(())
    } else {
        Err(format!("mpv command failed: {response}").into())
    }
}

#[cfg(windows)]
fn start_mpv(mpv: &Path, fixture: &Path, pipe: &str) -> io::Result<Child> {
    Command::new(mpv)
        .args([
            "--no-config",
            "--force-window=no",
            "--vid=no",
            "--ao=null",
            "--idle=yes",
            "--keep-open=yes",
            "--pause=yes",
            "--term-playing-msg=",
            "--msg-level=all=warn",
        ])
        .arg(format!("--input-ipc-server={pipe}"))
        .arg(fixture)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(windows)]
fn temporary_wav_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "apricot-rust-mpv-{}-{timestamp:x}.wav",
        std::process::id()
    ))
}

#[cfg(windows)]
fn write_silent_stereo_wav(path: &Path, sample_rate: u32, seconds: u32) -> io::Result<()> {
    let channels = 2_u16;
    let bits_per_sample = 16_u16;
    let bytes_per_sample = u32::from(bits_per_sample / 8);
    let data_size = sample_rate * seconds * u32::from(channels) * bytes_per_sample;
    let mut file = fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    let byte_rate = sample_rate * u32::from(channels) * bytes_per_sample;
    file.write_all(&byte_rate.to_le_bytes())?;
    let block_align = channels * (bits_per_sample / 8);
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;
    io::copy(&mut io::repeat(0).take(u64::from(data_size)), &mut file)?;
    file.flush()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The mpv IPC spike currently qualifies the Windows transport only.");
}
