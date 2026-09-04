//! Bounded Windows named-pipe transport for mpv JSON IPC.

#![allow(unsafe_code)]

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use windows::Win32::{Foundation::HANDLE, System::Pipes::PeekNamedPipe};

use crate::PlaybackError;

pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_READ_CHUNK_BYTES: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
static PIPE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
        loop {
            if Instant::now() >= deadline {
                return Err(PlaybackError::Timeout);
            }
            let available = available_bytes(&pipe)?;
            if available == 0 {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            let read_size = available.min(MAX_READ_CHUNK_BYTES);
            let mut chunk = vec![0_u8; read_size];
            let bytes_read = pipe.read(&mut chunk).map_err(|error| {
                PlaybackError::Operation(format!("mpv IPC read failed: {error}"))
            })?;
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

    fn open_before(&self, deadline: Instant) -> Result<File, PlaybackError> {
        loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.pipe_path)
            {
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
    format!(
        r"\\.\pipe\apricotplayer-rust-{}-{timestamp:x}-{sequence:x}",
        std::process::id()
    )
}

pub(crate) fn available_bytes(pipe: &File) -> Result<usize, PlaybackError> {
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
