//! Offline qualification of `yt-dlp`, `Node.js`, `FFmpeg`, `ffprobe`, and JSON parsing.

use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use apricot_platform::{
    DownloadEvent, DownloadMode, DownloadOptions, DownloadPhase, DownloadRequest, YtDlpDownloader,
};

const MAX_PROCESS_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..");
    let yt_dlp = repository
        .join("rust")
        .join(".cargo-local")
        .join("yt-dlp")
        .join("yt-dlp.exe");
    let ffmpeg = repository.join("vendor").join("ffmpeg").join("ffmpeg.exe");
    let ffprobe = [
        repository.join("vendor").join("ffmpeg").join("ffprobe.exe"),
        repository
            .join("release-dist")
            .join("_internal")
            .join("ffmpeg")
            .join("ffprobe.exe"),
        repository
            .join("dist")
            .join("ApricotPlayer")
            .join("_internal")
            .join("ffmpeg")
            .join("ffprobe.exe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
    .unwrap_or_else(|| PathBuf::from("ffprobe.exe"));
    let node = repository.join("vendor").join("node").join("node.exe");
    for executable in [&yt_dlp, &ffmpeg, &node] {
        if !executable.is_file() {
            return Err(format!("required tool was not found: {}", executable.display()).into());
        }
    }

    let version_start = Instant::now();
    let yt_dlp_version = checked_text(run(&yt_dlp, &["--version"])?)?;
    println!(
        "YT_DLP_VERSION={};START_MS={}",
        yt_dlp_version.trim(),
        version_start.elapsed().as_millis()
    );
    let node_version = checked_text(run(&node, &["--version"])?)?;
    println!("NODE_VERSION={}", node_version.trim());

    let wav = silent_stereo_wav(44_100, 2);
    let fixture = TemporaryFixture::create(wav.as_slice())?;
    let server = FixtureServer::start(wav)?;
    let media_url = server.url();
    qualify_extraction(&yt_dlp, &media_url)?;
    qualify_download(&yt_dlp, &ffmpeg, &media_url)?;
    qualify_probe_and_decode(&ffprobe, &ffmpeg, fixture.path())?;

    drop(server);
    println!("MEDIA_PROCESS_SPIKE=PASS");
    Ok(())
}

fn qualify_extraction(yt_dlp: &Path, media_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let extraction = run(
        yt_dlp,
        &[
            "--ignore-config",
            "--no-plugin-dirs",
            "--no-warnings",
            "--no-playlist",
            "--skip-download",
            "--dump-single-json",
            media_url,
        ],
    )?;
    let metadata: Value = serde_json::from_slice(&checked_output(extraction)?.stdout)?;
    if metadata.get("ext").and_then(Value::as_str) != Some("wav") {
        return Err(format!("yt-dlp returned unexpected metadata: {metadata}").into());
    }
    println!(
        "YT_DLP_DIRECT_EXTRACTION=PASS;ELAPSED_MS={}",
        started.elapsed().as_millis()
    );
    Ok(())
}

fn qualify_download(
    yt_dlp: &Path,
    ffmpeg: &Path,
    media_url: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = TemporaryDirectory::create("download")?;
    let request = DownloadRequest {
        url: media_url.to_owned(),
        title: "Offline fixture".to_owned(),
        mode: DownloadMode::Audio,
        output_directory: directory.path().to_owned(),
        target_path: None,
        allow_playlist: false,
        options: DownloadOptions {
            audio_format: "wav".to_owned(),
            ffmpeg_location: ffmpeg.parent().map(Path::to_owned),
            ..DownloadOptions::default()
        },
    };
    let mut events = Vec::new();
    let started = Instant::now();
    let summary = YtDlpDownloader::new(yt_dlp)?.download(
        &request,
        &Arc::new(AtomicBool::new(false)),
        |event| events.push(event),
    )?;
    let downloaded = summary
        .files
        .first()
        .filter(|path| path.is_file())
        .ok_or("production downloader did not report its completed file")?;
    if downloaded.extension().and_then(|value| value.to_str()) != Some("wav") {
        return Err(format!(
            "production downloader produced an unexpected path: {}",
            downloaded.display()
        )
        .into());
    }
    require_download_event(&events, "completion", |event| {
        matches!(event, DownloadEvent::FileFinished { .. })
    })?;
    require_download_event(&events, "download progress", |event| {
        matches!(
            event,
            DownloadEvent::Progress {
                phase: DownloadPhase::Downloading,
                ..
            }
        )
    })?;
    require_download_event(&events, "processing progress", |event| {
        matches!(
            event,
            DownloadEvent::Progress {
                phase: DownloadPhase::Processing,
                ..
            }
        )
    })?;
    println!(
        "YT_DLP_PRODUCTION_DOWNLOAD=PASS;EVENTS={};ELAPSED_MS={}",
        events.len(),
        started.elapsed().as_millis()
    );
    Ok(())
}

fn require_download_event(
    events: &[DownloadEvent],
    label: &str,
    predicate: impl Fn(&DownloadEvent) -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if events.iter().any(predicate) {
        Ok(())
    } else {
        Err(format!("production downloader did not emit {label}").into())
    }
}

fn qualify_probe_and_decode(
    ffprobe: &Path,
    ffmpeg: &Path,
    fixture: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture_text = fixture.to_string_lossy().into_owned();
    let probe_start = Instant::now();
    let probe = run(
        ffprobe,
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
            &fixture_text,
        ],
    )?;
    let metadata: Value = serde_json::from_slice(&checked_output(probe)?.stdout)?;
    let duration = metadata["format"]["duration"]
        .as_str()
        .ok_or("ffprobe did not return a duration")?
        .parse::<f64>()?;
    if (duration - 2.0).abs() > 0.01 {
        return Err(format!("ffprobe returned unexpected duration {duration}").into());
    }
    println!(
        "FFPROBE=PASS;ELAPSED_MS={}",
        probe_start.elapsed().as_millis()
    );

    let decode_start = Instant::now();
    checked_output(run(
        ffmpeg,
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &fixture_text,
            "-f",
            "null",
            "-",
        ],
    )?)?;
    println!(
        "FFMPEG_DECODE=PASS;ELAPSED_MS={}",
        decode_start.elapsed().as_millis()
    );
    Ok(())
}

fn run(executable: &Path, arguments: &[&str]) -> std::io::Result<Output> {
    let mut command = Command::new(executable);
    command.args(arguments);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.output()
}

fn checked_output(output: Output) -> Result<Output, Box<dyn std::error::Error>> {
    let total_size = output.stdout.len().saturating_add(output.stderr.len());
    if total_size > MAX_PROCESS_OUTPUT_BYTES {
        return Err("media tool output exceeded 8 MiB".into());
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(format!("media tool failed with {}: {message}", output.status).into());
    }
    Ok(output)
}

fn checked_text(output: Output) -> Result<String, Box<dyn std::error::Error>> {
    Ok(String::from_utf8(checked_output(output)?.stdout)?)
}

struct FixtureServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl FixtureServer {
    fn start(body: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let body = Arc::new(body);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => serve_fixture(stream, &body),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            stop,
            worker: Some(worker),
        })
    }

    fn url(&self) -> String {
        format!("http://{}/fixture.wav", self.address)
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve_fixture(mut stream: TcpStream, body: &[u8]) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut request = [0_u8; 4096];
    let _ = stream.read(&mut request);
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn temporary_wav_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "apricot-rust-process-{}-{timestamp:x}.wav",
        std::process::id()
    ))
}

struct TemporaryFixture {
    path: PathBuf,
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn create(label: &str) -> std::io::Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "apricot-rust-{label}-{}-{timestamp:x}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl TemporaryFixture {
    fn create(contents: &[u8]) -> std::io::Result<Self> {
        let path = temporary_wav_path();
        fs::write(&path, contents)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn silent_stereo_wav(sample_rate: u32, seconds: u32) -> Vec<u8> {
    let channels = 2_u16;
    let bits_per_sample = 16_u16;
    let bytes_per_sample = u32::from(bits_per_sample / 8);
    let data_size = sample_rate * seconds * u32::from(channels) * bytes_per_sample;
    let mut wav = Vec::with_capacity(usize::try_from(data_size).unwrap_or(0).saturating_add(44));
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * u32::from(channels) * bytes_per_sample;
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&(channels * (bits_per_sample / 8)).to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.resize(wav.capacity(), 0);
    wav
}
