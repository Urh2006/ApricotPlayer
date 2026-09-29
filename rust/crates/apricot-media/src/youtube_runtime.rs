//! Bounded worker that owns a persistent `YouTube` component off the UI thread.

use std::{
    sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel},
    thread::{self, JoinHandle},
};

use thiserror::Error;

use crate::{YoutubeCommand, YoutubeErrorCode, YoutubeResponsePayload};

const REQUEST_CAPACITY: usize = 16;
const UPDATE_CAPACITY: usize = 32;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct YoutubeEngineError {
    pub message: String,
    pub restart_required: bool,
    pub code: Option<YoutubeErrorCode>,
}

impl YoutubeEngineError {
    pub fn new(message: impl Into<String>, restart_required: bool) -> Self {
        Self {
            message: message.into(),
            restart_required,
            code: None,
        }
    }

    pub fn backend(
        code: YoutubeErrorCode,
        message: impl Into<String>,
        restart_required: bool,
    ) -> Self {
        Self {
            message: message.into(),
            restart_required,
            code: Some(code),
        }
    }
}

pub trait YoutubeEngine: Send {
    /// Executes one protocol command against the persistent component.
    ///
    /// # Errors
    ///
    /// Returns a classified error indicating whether the component process must
    /// be recreated before the next request.
    fn execute(
        &mut self,
        command: YoutubeCommand,
    ) -> Result<YoutubeResponsePayload, YoutubeEngineError>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct YoutubeUpdate {
    pub generation: u64,
    pub result: Result<YoutubeResponsePayload, YoutubeEngineError>,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum YoutubeRuntimeError {
    #[error("YouTube worker is busy")]
    Busy,
    #[error("YouTube worker has stopped")]
    Stopped,
    #[error("could not create YouTube worker: {0}")]
    Spawn(String),
}

enum RuntimeRequest {
    Execute {
        generation: u64,
        command: YoutubeCommand,
    },
    Shutdown,
}

type EngineFactory =
    Box<dyn FnMut() -> Result<Box<dyn YoutubeEngine>, YoutubeEngineError> + Send + 'static>;

pub struct YoutubeRuntime {
    requests: SyncSender<RuntimeRequest>,
    updates: Receiver<YoutubeUpdate>,
    worker: Option<JoinHandle<()>>,
}

impl YoutubeRuntime {
    /// Creates a lazy bounded worker. No helper process is started until the
    /// first command, so application startup remains unaffected.
    ///
    /// # Errors
    ///
    /// Returns an error only when the operating system cannot create the worker.
    pub fn spawn(factory: EngineFactory) -> Result<Self, YoutubeRuntimeError> {
        let (requests, request_receiver) = sync_channel(REQUEST_CAPACITY);
        let (update_sender, updates) = sync_channel(UPDATE_CAPACITY);
        let worker = thread::Builder::new()
            .name("apricot-youtube-worker".to_owned())
            .spawn(move || youtube_worker(&request_receiver, &update_sender, factory))
            .map_err(|error| YoutubeRuntimeError::Spawn(error.to_string()))?;
        Ok(Self {
            requests,
            updates,
            worker: Some(worker),
        })
    }

    /// Enqueues work without blocking the caller.
    ///
    /// # Errors
    ///
    /// Returns [`YoutubeRuntimeError::Busy`] when the bounded queue is full, or
    /// [`YoutubeRuntimeError::Stopped`] after worker shutdown.
    pub fn execute(
        &self,
        generation: u64,
        command: YoutubeCommand,
    ) -> Result<(), YoutubeRuntimeError> {
        self.requests
            .try_send(RuntimeRequest::Execute {
                generation,
                command,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => YoutubeRuntimeError::Busy,
                TrySendError::Disconnected(_) => YoutubeRuntimeError::Stopped,
            })
    }

    /// Returns one currently buffered update without blocking.
    ///
    /// # Errors
    ///
    /// Returns [`YoutubeRuntimeError::Stopped`] after worker shutdown.
    pub fn poll_update(&self) -> Result<Option<YoutubeUpdate>, YoutubeRuntimeError> {
        match self.updates.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(YoutubeRuntimeError::Stopped),
        }
    }
}

impl Drop for YoutubeRuntime {
    fn drop(&mut self) {
        let _ = self.requests.send(RuntimeRequest::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn youtube_worker(
    requests: &Receiver<RuntimeRequest>,
    updates: &SyncSender<YoutubeUpdate>,
    mut factory: EngineFactory,
) {
    let mut engine: Option<Box<dyn YoutubeEngine>> = None;
    let mut session_config: Option<YoutubeCommand> = None;
    while let Ok(request) = requests.recv() {
        let RuntimeRequest::Execute {
            generation,
            command,
        } = request
        else {
            break;
        };
        if matches!(command, YoutubeCommand::Configure { .. }) {
            session_config = Some(command.clone());
        }
        let result =
            execute_with_lazy_engine(&mut engine, &mut factory, session_config.as_ref(), command);
        if result.as_ref().is_err_and(|error| error.restart_required) {
            engine = None;
        }
        let _ = updates.try_send(YoutubeUpdate { generation, result });
    }
}

fn execute_with_lazy_engine(
    engine: &mut Option<Box<dyn YoutubeEngine>>,
    factory: &mut EngineFactory,
    session_config: Option<&YoutubeCommand>,
    command: YoutubeCommand,
) -> Result<YoutubeResponsePayload, YoutubeEngineError> {
    let was_missing = engine.is_none();
    let active = if let Some(active) = engine.as_mut() {
        active
    } else {
        engine.insert(factory()?)
    };
    if was_missing
        && !matches!(command, YoutubeCommand::Configure { .. })
        && let Some(config) = session_config
    {
        active.execute(config.clone())?;
    }
    active.execute(command)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use super::{YoutubeEngine, YoutubeEngineError, YoutubeRuntime, YoutubeUpdate};
    use crate::{YoutubeCommand, YoutubeResponsePayload, YoutubeSessionConfig};

    struct FakeEngine {
        commands: Arc<Mutex<Vec<YoutubeCommand>>>,
        fail_transport_once: bool,
    }

    impl YoutubeEngine for FakeEngine {
        fn execute(
            &mut self,
            command: YoutubeCommand,
        ) -> Result<YoutubeResponsePayload, YoutubeEngineError> {
            self.commands.lock().expect("commands").push(command);
            if self.fail_transport_once {
                self.fail_transport_once = false;
                return Err(YoutubeEngineError::new("pipe failed", true));
            }
            Ok(YoutubeResponsePayload::Configured)
        }
    }

    fn wait_for_update(runtime: &YoutubeRuntime) -> YoutubeUpdate {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(update) = runtime.poll_update().expect("runtime update") {
                return update;
            }
            assert!(Instant::now() < deadline, "runtime update timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn runtime_is_lazy_and_tags_responses_with_generation() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&factory_calls);
        let commands = Arc::new(Mutex::new(Vec::new()));
        let fake_commands = Arc::clone(&commands);
        let runtime = YoutubeRuntime::spawn(Box::new(move || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(FakeEngine {
                commands: Arc::clone(&fake_commands),
                fail_transport_once: false,
            }))
        }))
        .expect("runtime");
        assert_eq!(factory_calls.load(Ordering::Relaxed), 0);

        runtime.execute(21, YoutubeCommand::Hello).expect("execute");
        assert_eq!(
            wait_for_update(&runtime),
            YoutubeUpdate {
                generation: 21,
                result: Ok(YoutubeResponsePayload::Configured),
            }
        );
        assert_eq!(factory_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn transport_restart_replays_latest_configuration() {
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let calls = Arc::clone(&factory_calls);
        let commands = Arc::new(Mutex::new(Vec::new()));
        let fake_commands = Arc::clone(&commands);
        let runtime = YoutubeRuntime::spawn(Box::new(move || {
            let call = calls.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(FakeEngine {
                commands: Arc::clone(&fake_commands),
                fail_transport_once: call == 0,
            }))
        }))
        .expect("runtime");
        let config = YoutubeCommand::Configure {
            config: YoutubeSessionConfig {
                cookies_header: Some("PREF=test".to_owned()),
                cookies_file: None,
                proxy_url: None,
                ..YoutubeSessionConfig::default()
            },
        };
        runtime.execute(1, config.clone()).expect("configure");
        assert!(wait_for_update(&runtime).result.is_err());
        runtime
            .execute(2, YoutubeCommand::Hello)
            .expect("hello after restart");
        assert!(wait_for_update(&runtime).result.is_ok());

        assert_eq!(factory_calls.load(Ordering::Relaxed), 2);
        let commands = commands.lock().expect("commands");
        assert_eq!(
            &*commands,
            &vec![config.clone(), config, YoutubeCommand::Hello]
        );
    }
}
