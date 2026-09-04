//! Synchronous process boundary for the independently replaceable Rust
//! `YouTube` helper. Callers own threading and cancellation policy.

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use apricot_media::{
    MAX_YOUTUBE_MESSAGE_BYTES, YOUTUBE_HELPER_PROTOCOL_VERSION, YoutubeCommand, YoutubeEngine,
    YoutubeEngineError, YoutubeHelperError, YoutubeRequest, YoutubeResponse,
    YoutubeResponsePayload,
};
use thiserror::Error;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CLOSE_GRACE_PERIOD: Duration = Duration::from_millis(500);

#[derive(Debug, Error)]
pub enum YoutubeProcessError {
    #[error("could not launch Rust YouTube helper: {0}")]
    Launch(String),
    #[error("YouTube helper transport failed: {0}")]
    Transport(String),
    #[error("YouTube helper protocol failed: {0}")]
    Protocol(String),
    #[error("YouTube helper request failed: {0}")]
    Helper(#[from] YoutubeHelperError),
}

pub struct YoutubeHelperProcess {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_request_id: u64,
    helper_version: String,
    backend_revision: String,
    closed: bool,
}

impl YoutubeHelperProcess {
    /// Starts the helper and validates its protocol handshake.
    ///
    /// # Errors
    ///
    /// Returns an error when the executable cannot be started, its pipes cannot
    /// be created, or the helper reports an incompatible protocol.
    pub fn start(executable: &Path) -> Result<Self, YoutubeProcessError> {
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command
            .spawn()
            .map_err(|error| YoutubeProcessError::Launch(error.to_string()))?;
        let input = child.stdin.take().ok_or_else(|| {
            YoutubeProcessError::Launch("helper stdin pipe was not created".to_owned())
        })?;
        let output = child.stdout.take().ok_or_else(|| {
            YoutubeProcessError::Launch("helper stdout pipe was not created".to_owned())
        })?;
        let mut process = Self {
            child,
            input,
            output: BufReader::new(output),
            next_request_id: 1,
            helper_version: String::new(),
            backend_revision: String::new(),
            closed: false,
        };
        let response = process.request(YoutubeCommand::Hello)?;
        let YoutubeResponsePayload::Hello {
            helper_version,
            backend_revision,
            ..
        } = response
        else {
            return Err(YoutubeProcessError::Protocol(
                "helper did not return a hello response".to_owned(),
            ));
        };
        process.helper_version = helper_version;
        process.backend_revision = backend_revision;
        Ok(process)
    }

    pub fn helper_version(&self) -> &str {
        &self.helper_version
    }

    pub fn backend_revision(&self) -> &str {
        &self.backend_revision
    }

    /// Sends one request over the existing helper process.
    ///
    /// # Errors
    ///
    /// Returns an error for broken pipes, malformed or mismatched responses, or
    /// a structured backend failure.
    pub fn request(
        &mut self,
        command: YoutubeCommand,
    ) -> Result<YoutubeResponsePayload, YoutubeProcessError> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let request = YoutubeRequest::new(request_id, command);
        let encoded = serde_json::to_vec(&request)
            .map_err(|error| YoutubeProcessError::Protocol(error.to_string()))?;
        if encoded.len() > MAX_YOUTUBE_MESSAGE_BYTES {
            return Err(YoutubeProcessError::Protocol(format!(
                "request exceeded the {MAX_YOUTUBE_MESSAGE_BYTES}-byte protocol limit"
            )));
        }
        self.input
            .write_all(&encoded)
            .and_then(|()| self.input.write_all(b"\n"))
            .and_then(|()| self.input.flush())
            .map_err(|error| YoutubeProcessError::Transport(error.to_string()))?;

        let line = read_bounded_line(&mut self.output)?;
        let response: YoutubeResponse = serde_json::from_slice(&line)
            .map_err(|error| YoutubeProcessError::Protocol(error.to_string()))?;
        validate_response(&response, request_id)?;
        match response.payload {
            YoutubeResponsePayload::Error { error } => Err(error.into()),
            payload => Ok(payload),
        }
    }

    /// Requests an orderly shutdown, then terminates an unresponsive helper.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        if self.child.try_wait().is_ok_and(|status| status.is_some()) {
            return;
        }
        let request = YoutubeRequest::new(self.next_request_id, YoutubeCommand::Shutdown);
        if let Ok(encoded) = serde_json::to_vec(&request) {
            let sent = self
                .input
                .write_all(&encoded)
                .and_then(|()| self.input.write_all(b"\n"))
                .and_then(|()| self.input.flush())
                .is_ok();
            if sent {
                let _ = read_bounded_line(&mut self.output);
            }
        }
        let deadline = Instant::now() + CLOSE_GRACE_PERIOD;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for YoutubeHelperProcess {
    fn drop(&mut self) {
        self.close();
    }
}

impl YoutubeEngine for YoutubeHelperProcess {
    fn execute(
        &mut self,
        command: YoutubeCommand,
    ) -> Result<YoutubeResponsePayload, YoutubeEngineError> {
        self.request(command).map_err(|error| {
            let restart_required = !matches!(error, YoutubeProcessError::Helper(_));
            YoutubeEngineError::new(error.to_string(), restart_required)
        })
    }
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Vec<u8>, YoutubeProcessError> {
    let mut line = Vec::new();
    let read = std::io::Read::take(&mut *reader, (MAX_YOUTUBE_MESSAGE_BYTES + 2) as u64)
        .read_until(b'\n', &mut line)
        .map_err(|error| YoutubeProcessError::Transport(error.to_string()))?;
    if read == 0 {
        return Err(YoutubeProcessError::Transport(
            "helper closed its output pipe".to_owned(),
        ));
    }
    if line.len() > MAX_YOUTUBE_MESSAGE_BYTES || !line.ends_with(b"\n") {
        return Err(YoutubeProcessError::Protocol(format!(
            "response exceeded the {MAX_YOUTUBE_MESSAGE_BYTES}-byte protocol limit"
        )));
    }
    line.pop();
    if line.ends_with(b"\r") {
        line.pop();
    }
    Ok(line)
}

fn validate_response(
    response: &YoutubeResponse,
    request_id: u64,
) -> Result<(), YoutubeProcessError> {
    if response.protocol_version != YOUTUBE_HELPER_PROTOCOL_VERSION {
        return Err(YoutubeProcessError::Protocol(format!(
            "helper uses protocol {}, but the app expects {}",
            response.protocol_version, YOUTUBE_HELPER_PROTOCOL_VERSION
        )));
    }
    if response.request_id != request_id {
        return Err(YoutubeProcessError::Protocol(format!(
            "helper response id {} did not match request id {request_id}",
            response.request_id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{YoutubeProcessError, read_bounded_line, validate_response};
    use apricot_media::{MAX_YOUTUBE_MESSAGE_BYTES, YoutubeResponse, YoutubeResponsePayload};
    use std::io::Cursor;

    #[test]
    fn response_identity_must_match_request() {
        let response = YoutubeResponse::success(8, YoutubeResponsePayload::Configured);
        assert!(matches!(
            validate_response(&response, 9),
            Err(YoutubeProcessError::Protocol(_))
        ));
    }

    #[test]
    fn response_reader_is_bounded() {
        let mut valid = Cursor::new(b"{}\n".to_vec());
        assert_eq!(read_bounded_line(&mut valid).expect("line"), b"{}");

        let mut oversized = Cursor::new(vec![b'x'; MAX_YOUTUBE_MESSAGE_BYTES + 2]);
        assert!(matches!(
            read_bounded_line(&mut oversized),
            Err(YoutubeProcessError::Protocol(_))
        ));
    }
}
