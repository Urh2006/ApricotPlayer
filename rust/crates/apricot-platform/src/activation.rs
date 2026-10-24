//! A second launch hands its request to the running instance (the Unix
//! counterpart of Windows `WM_COPYDATA` in `activation_win32.rs`): the
//! primary instance listens on a per-user Unix socket next to its
//! single-instance lock, and a second launch writes one request to it.

use std::{
    io::{self, Read, Write},
    os::unix::{ffi::OsStrExt, net::UnixListener, net::UnixStream},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::ApplicationIdentity;

/// The longest request accepted; a path never comes close.
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

/// What a second launch asks the running instance to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivationMessage {
    Show,
    OpenFile(PathBuf),
    OpenSettings,
}

impl ActivationMessage {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Show => b"show".to_vec(),
            Self::OpenSettings => b"settings".to_vec(),
            Self::OpenFile(path) => {
                let mut bytes = b"open ".to_vec();
                bytes.extend_from_slice(path.as_os_str().as_bytes());
                bytes
            }
        }
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes {
            b"show" => Some(Self::Show),
            b"settings" => Some(Self::OpenSettings),
            _ => {
                let path = bytes.strip_prefix(b"open ")?;
                (!path.is_empty())
                    .then(|| Self::OpenFile(PathBuf::from(std::ffi::OsStr::from_bytes(path))))
            }
        }
    }
}

/// The socket of `identity` in the per-user temporary folder, beside the
/// single-instance lock.
#[must_use]
pub fn socket_path(identity: ApplicationIdentity) -> PathBuf {
    let name = identity.folder_names().0;
    let path = std::env::temp_dir().join(format!("{name}.sock"));
    // A Unix socket path is limited to about 104 bytes; a long temporary
    // folder falls back to /tmp with the user name in the file name.
    if path.as_os_str().len() < 100 {
        return path;
    }
    let user = std::env::var("USER").unwrap_or_default();
    PathBuf::from("/tmp").join(format!("{name}-{user}.sock"))
}

/// Sends `message` to the running instance.
///
/// # Errors
///
/// Returns the I/O error when no instance listens or the write fails.
pub fn forward(identity: ApplicationIdentity, message: &ActivationMessage) -> io::Result<()> {
    forward_to(&socket_path(identity), message)
}

fn forward_to(path: &Path, message: &ActivationMessage) -> io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&message.encode())?;
    stream.shutdown(std::net::Shutdown::Write)
}

/// The primary instance's listener; the socket file is removed on drop.
pub struct ActivationListener {
    path: PathBuf,
}

impl Drop for ActivationListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Listens for requests of second launches and calls `on_message` for each,
/// on a background thread. Only the instance that holds the single-instance
/// lock calls this, so a leftover socket of a crashed instance is replaced.
///
/// # Errors
///
/// Returns the I/O error when the socket cannot be created.
pub fn listen(
    identity: ApplicationIdentity,
    on_message: impl Fn(ActivationMessage) + Send + 'static,
) -> io::Result<ActivationListener> {
    listen_at(socket_path(identity), on_message)
}

fn listen_at(
    path: PathBuf,
    on_message: impl Fn(ActivationMessage) + Send + 'static,
) -> io::Result<ActivationListener> {
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::thread::Builder::new()
        .name("activation".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut bytes = Vec::new();
                if stream
                    .take(MAX_REQUEST_BYTES)
                    .read_to_end(&mut bytes)
                    .is_ok()
                    && let Some(message) = ActivationMessage::decode(&bytes)
                {
                    on_message(message);
                }
            }
        })?;
    Ok(ActivationListener { path })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::mpsc, time::Duration};

    use super::{ActivationMessage, forward_to, listen_at};

    #[test]
    fn requests_round_trip_including_paths_with_spaces() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("test.sock");
        let (sender, receiver) = mpsc::channel();
        let listener = listen_at(path.clone(), move |message| {
            let _ = sender.send(message);
        })
        .expect("listen");
        for message in [
            ActivationMessage::Show,
            ActivationMessage::OpenSettings,
            ActivationMessage::OpenFile(PathBuf::from("/Users/a b/Glasba/pesem čžš.mp3")),
        ] {
            forward_to(&path, &message).expect("forward");
            assert_eq!(
                receiver
                    .recv_timeout(Duration::from_secs(5))
                    .expect("received"),
                message
            );
        }
        drop(listener);
        assert!(!path.exists());
        assert!(forward_to(&path, &ActivationMessage::Show).is_err());
    }

    #[test]
    fn unknown_requests_are_ignored() {
        assert_eq!(ActivationMessage::decode(b"open "), None);
        assert_eq!(ActivationMessage::decode(b"delete everything"), None);
    }
}
