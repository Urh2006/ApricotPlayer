//! WASAPI endpoint and process-loopback capture. COM objects stay on one worker.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
#![allow(unsafe_code)]

use super::{CAPTURE_RATE, CaptureKind, CaptureQueue, CaptureSource};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

struct Com;
impl Com {
    fn enter() -> Result<Self, String> {
        wasapi::initialize_mta()
            .ok()
            .map_err(|error| error.to_string())?;
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        wasapi::deinitialize()
    }
}

pub(super) fn sources() -> Result<Vec<CaptureSource>, String> {
    let _com = Com::enter()?;
    let enumerator = DeviceEnumerator::new().map_err(|error| error.to_string())?;
    let mut sources = Vec::new();
    let mut applications = BTreeSet::new();
    for direction in [Direction::Capture, Direction::Render] {
        let collection = enumerator
            .get_device_collection(direction)
            .map_err(|error| error.to_string())?;
        for index in 0..collection
            .get_nbr_devices()
            .map_err(|error| error.to_string())?
        {
            let device = collection
                .get_device_at_index(index)
                .map_err(|error| error.to_string())?;
            let id = device.get_id().map_err(|error| error.to_string())?;
            let name = device.get_friendlyname().unwrap_or_else(|_| id.clone());
            let kind = if direction == Direction::Capture {
                CaptureKind::Input { device: id }
            } else {
                CaptureKind::Output { device: id }
            };
            sources.push(CaptureSource {
                name: format!(
                    "{}: {name}",
                    if direction == Direction::Capture {
                        "Input"
                    } else {
                        "Computer audio"
                    }
                ),
                kind,
                gain: 1.0,
            });
            if direction == Direction::Render
                && let Ok(manager) = device.get_iaudiosessionmanager()
                && let Ok(sessions) = manager.get_audiosessionenumerator()
            {
                for index in 0..sessions.get_count().unwrap_or_default() {
                    if let Ok(session) = sessions.get_session(index)
                        && let Ok(pid) = session.get_process_id()
                        && pid > 0
                        && applications.insert(pid)
                    {
                        let identity = process_path(pid).unwrap_or_default();
                        let name = session
                            .get_display_name()
                            .ok()
                            .filter(|name| !name.is_empty())
                            .unwrap_or_else(|| {
                                std::path::Path::new(&identity).file_name().map_or_else(
                                    || format!("Process {pid}"),
                                    |name| name.to_string_lossy().into_owned(),
                                )
                            });
                        sources.push(CaptureSource {
                            name: format!("Application: {name}"),
                            kind: CaptureKind::Application {
                                process_id: pid,
                                identity,
                                exclude: false,
                            },
                            gain: 1.0,
                        });
                    }
                }
            }
        }
    }
    Ok(sources)
}

pub(super) fn capture(
    source: &CaptureSource,
    queue: Arc<CaptureQueue>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let result = capture_inner(source, queue, stop, &ready);
    if let Err(error) = &result {
        let _ = ready.try_send(Err(error.clone()));
    }
    result
}

fn capture_inner(
    source: &CaptureSource,
    queue: Arc<CaptureQueue>,
    stop: Arc<AtomicBool>,
    ready: &SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let _com = Com::enter()?;
    let mut client = match &source.kind {
        CaptureKind::Input { device } | CaptureKind::Output { device } => DeviceEnumerator::new()
            .and_then(|enumerator| enumerator.get_device(device))
            .and_then(|device| device.get_iaudioclient())
            .map_err(|error| error.to_string())?,
        CaptureKind::Application {
            process_id,
            identity,
            exclude,
        } => {
            let actual = process_path(*process_id);
            let pid = if identity.is_empty()
                || actual
                    .as_ref()
                    .is_some_and(|path| path.eq_ignore_ascii_case(identity))
            {
                *process_id
            } else {
                sources()?.iter().find_map(|candidate|match &candidate.kind {CaptureKind::Application {process_id,identity:other,..} if other.eq_ignore_ascii_case(identity)=>Some(*process_id),_=>None}).ok_or("The selected application is no longer running. Choose it again in Recording.")?
            };
            AudioClient::new_application_loopback_client(pid, !exclude).map_err(|error| {
                format!("Process capture requires Windows 10 build 20348 or later. {error}")
            })?
        }
    };
    let format = WaveFormat::new(32, 32, SampleType::Float, CAPTURE_RATE as usize, 2, None);
    client
        .initialize_client(
            &format,
            Direction::Capture,
            StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000,
            },
            None,
        )
        .map_err(|error| error.to_string())?;
    let event = client
        .set_get_eventhandle()
        .map_err(|error| error.to_string())?;
    let capture = client
        .get_audiocaptureclient()
        .map_err(|error| error.to_string())?;
    client.start_stream().map_err(|error| error.to_string())?;
    if ready.send(Ok(())).is_err() {
        let _ = client.stop_stream();
        return Ok(());
    }
    let mut bytes = VecDeque::new();
    while !stop.load(Ordering::Acquire) {
        if capture
            .get_next_packet_size()
            .map_err(|error| error.to_string())?
            .unwrap_or(0)
            > 0
        {
            capture
                .read_from_device_to_deque(&mut bytes)
                .map_err(|error| error.to_string())?;
            let contiguous = bytes.make_contiguous();
            queue.push(
                contiguous
                    .chunks_exact(4)
                    .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap_or_default())),
            );
            bytes.clear();
        }
        let _ = event.wait_for_event(50);
    }
    client.stop_stream().map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_millis(1));
    Ok(())
}

fn process_path(pid: u32) -> Option<String> {
    use windows::{
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
        },
        core::PWSTR,
    };
    // SAFETY: The handle is query-only and always closed; the output points to
    // a live UTF-16 buffer whose length is passed to Windows.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 32_768];
        let mut length = u32::try_from(buffer.len()).ok()?;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}
