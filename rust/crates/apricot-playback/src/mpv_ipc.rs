//! Bounded mpv JSON IPC transport: a named pipe on Windows and a Unix-domain
//! socket elsewhere.

#![cfg_attr(windows, allow(unsafe_code))]

#[cfg(windows)]
use std::{
    fs::{File, OpenOptions},
    os::windows::io::AsRawHandle,
};
#[cfg(unix)]
use std::{io::ErrorKind, os::unix::net::UnixStream};
use std::{
    io::{Read as _, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
#[cfg(windows)]
use windows::Win32::{Foundation::HANDLE, System::Pipes::PeekNamedPipe};

use crate::PlaybackError;

pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_READ_CHUNK_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
static PIPE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// One open connection to mpv's IPC server.
#[cfg(windows)]
pub(crate) type IpcStream = File;
/// One open connection to mpv's IPC server.
#[cfg(unix)]
pub(crate) type IpcStream = UnixStream;

#[derive(Clone, Debug)]
pub struct MpvIpcClient {
    pipe_path: String,
    request_lock: Arc<Mutex<()>>,
}

impl MpvIpcClient {
    pub fn new(pipe_path: impl Into<String>) -> Self {
        Self {
            pipe_path: pipe_path.into(),
            request_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn pipe_path(&self) -> &str {
        &self.pipe_path
    }

    /// Sends one command and returns the matching mpv response.
    ///
    /// Unsolicited events and responses for other request IDs are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackError`] when the pipe cannot be opened before the
    /// deadline, I/O fails, the response is too large or malformed, or mpv does
    /// not return a matching response in time.
    pub fn request(&self, command: Value, timeout: Duration) -> Result<Value, PlaybackError> {
        let _request_guard = self.request_lock.lock().map_err(|_| {
            PlaybackError::Operation("mpv IPC request lock was poisoned".to_owned())
        })?;
        let deadline = Instant::now() + timeout;
        let mut pipe = self.open_before(deadline)?;
        let request_id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut envelope = serde_json::Map::new();
        envelope.insert("command".to_owned(), command);
        envelope.insert("request_id".to_owned(), json!(request_id));
        let mut payload = serde_json::to_vec(&envelope)
            .map_err(|error| PlaybackError::InvalidData(error.to_string()))?;
        payload.push(b'\n');
        pipe.write_all(&payload)
            .and_then(|()| pipe.flush())
            .map_err(|error| PlaybackError::Operation(format!("mpv IPC write failed: {error}")))?;

        let mut response_buffer = Vec::new();
        let mut chunk = vec![0_u8; MAX_READ_CHUNK_BYTES];
        loop {
            if Instant::now() >= deadline {
                return Err(PlaybackError::Timeout);
            }
            let bytes_read = read_available(&mut pipe, &mut chunk)?;
            if bytes_read == 0 {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            response_buffer.extend_from_slice(&chunk[..bytes_read]);
            if response_buffer.len() > MAX_RESPONSE_BYTES {
                return Err(PlaybackError::InvalidData(
                    "mpv IPC response exceeded 1 MiB".to_owned(),
                ));
            }
            if let Some(response) = take_matching_response(&mut response_buffer, request_id)? {
                return Ok(response);
            }
        }
    }

    fn open_before(&self, deadline: Instant) -> Result<IpcStream, PlaybackError> {
        loop {
            match open_ipc_stream(&self.pipe_path) {
                Ok(pipe) => return Ok(pipe),
                Err(error) if Instant::now() >= deadline => {
                    return Err(PlaybackError::Operation(format!(
                        "mpv IPC pipe was unavailable: {error}"
                    )));
                }
                Err(_) => {}
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl PartialEq for MpvIpcClient {
    fn eq(&self, other: &Self) -> bool {
        self.pipe_path == other.pipe_path
    }
}

impl Eq for MpvIpcClient {}

pub fn make_unique_ipc_path() -> String {
    let sequence = PIPE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let name = format!(
        "apricotplayer-rust-{}-{timestamp:x}-{sequence:x}",
        std::process::id()
    );
    #[cfg(windows)]
    return format!(r"\\.\pipe\{name}");
    // Socket paths are limited to about 100 bytes, so the short temporary
    // folder is used rather than a per-user cache folder.
    #[cfg(unix)]
    {
        let path = std::env::temp_dir().join(format!("{name}.sock"));
        // A Unix socket path is limited to about 104 bytes.
        let path = if path.as_os_str().len() < 100 {
            path
        } else {
            std::path::PathBuf::from("/tmp").join(format!("{name}.sock"))
        };
        path.to_string_lossy().into_owned()
    }
}

/// Opens a connection to the IPC server at `path`.
#[cfg(windows)]
pub(crate) fn open_ipc_stream(path: &str) -> std::io::Result<IpcStream> {
    OpenOptions::new().read(true).write(true).open(path)
}

/// Opens a non-blocking connection to the IPC server at `path`, so
/// [`read_available`] never waits.
#[cfg(unix)]
pub(crate) fn open_ipc_stream(path: &str) -> std::io::Result<IpcStream> {
    let stream = UnixStream::connect(path)?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}

/// Reads whatever the server has sent, at most `buffer.len()` bytes, without
/// waiting; `Ok(0)` means nothing is available yet.
#[cfg(unix)]
pub(crate) fn read_available(
    stream: &mut IpcStream,
    buffer: &mut [u8],
) -> Result<usize, PlaybackError> {
    match stream.read(buffer) {
        // The server closed the socket, as a broken pipe does on Windows.
        Ok(0) => Err(PlaybackError::Operation(
            "mpv IPC read failed: the connection was closed".to_owned(),
        )),
        Ok(read) => Ok(read),
        Err(error) if error.kind() == ErrorKind::WouldBlock => Ok(0),
        Err(error) => Err(PlaybackError::Operation(format!(
            "mpv IPC read failed: {error}"
        ))),
    }
}

/// Reads whatever the server has sent, at most `buffer.len()` bytes, without
/// waiting; `Ok(0)` means nothing is available yet.
#[cfg(windows)]
pub(crate) fn read_available(
    pipe: &mut IpcStream,
    buffer: &mut [u8],
) -> Result<usize, PlaybackError> {
    let available = available_bytes(pipe)?;
    if available == 0 {
        return Ok(0);
    }
    let read_size = available.min(buffer.len());
    pipe.read(&mut buffer[..read_size])
        .map_err(|error| PlaybackError::Operation(format!("mpv IPC read failed: {error}")))
}

#[cfg(windows)]
fn available_bytes(pipe: &IpcStream) -> Result<usize, PlaybackError> {
    let mut available = 0_u32;
    // SAFETY: `pipe` owns a live Windows file handle for this call. The only
    // output pointer references `available`, which remains valid and writable.
    unsafe {
        PeekNamedPipe(
            HANDLE(pipe.as_raw_handle()),
            None,
            0,
            None,
            Some(&raw mut available),
            None,
        )
    }
    .map_err(|error| PlaybackError::Operation(format!("mpv IPC peek failed: {error}")))?;
    usize::try_from(available)
        .map_err(|_| PlaybackError::InvalidData("mpv IPC byte count overflowed".to_owned()))
}

fn take_matching_response(
    buffer: &mut Vec<u8>,
    request_id: u64,
) -> Result<Option<Value>, PlaybackError> {
    while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
        let line: Vec<u8> = buffer.drain(..=newline).collect();
        let line = &line[..line.len().saturating_sub(1)];
        if line.is_empty() {
            continue;
        }
        let response: Value = serde_json::from_slice(line)
            .map_err(|error| PlaybackError::InvalidData(error.to_string()))?;
        if response.get("request_id").and_then(Value::as_u64) == Some(request_id) {
            return Ok(Some(response));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::take_matching_response;

    /// A real mpv answers over the platform transport (named pipe or Unix
    /// socket) and a closed server is reported instead of waited for.
    #[test]
    #[ignore = "requires APRICOT_TEST_MPV_EXECUTABLE"]
    fn real_mpv_answers_over_the_platform_transport() {
        let executable = std::env::var_os("APRICOT_TEST_MPV_EXECUTABLE").expect("mpv");
        let path = super::make_unique_ipc_path();
        let mut mpv = std::process::Command::new(executable)
            .args(["--idle=yes", "--no-config", "--no-video", "--ao=null"])
            .arg(format!("--input-ipc-server={path}"))
            .spawn()
            .expect("start mpv");
        let client = super::MpvIpcClient::new(path.clone());
        let response = client
            .request(
                json!(["get_property", "idle-active"]),
                std::time::Duration::from_secs(10),
            )
            .expect("response");
        assert_eq!(response["data"], json!(true));
        let mut stream = super::open_ipc_stream(&path).expect("second connection");
        mpv.kill().expect("stop mpv");
        mpv.wait().expect("mpv exit");
        let mut buffer = [0_u8; 64];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let closed = loop {
            match super::read_available(&mut stream, &mut buffer) {
                Err(_) => break true,
                Ok(_) if std::time::Instant::now() >= deadline => break false,
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        };
        assert!(closed, "a closed mpv was not reported");
        #[cfg(unix)]
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ignores_events_and_returns_only_the_matching_response() {
        let mut data = br#"{"event":"file-loaded"}
{"request_id":3,"error":"success"}
{"request_id":7,"error":"success","data":42}
"#
        .to_vec();
        let response = take_matching_response(&mut data, 7)
            .expect("valid JSON")
            .expect("matching response");
        assert_eq!(response["data"], json!(42));
    }
}
