//! ScreenCaptureKit audio-only streams, including microphone and app filters.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use super::{CAPTURE_RATE, CaptureKind, CaptureQueue, CaptureSource};
use screencapturekit::prelude::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};

pub(super) fn sources() -> Result<Vec<CaptureSource>, String> {
    let mut sources = vec![CaptureSource {
        name: String::new(),
        kind: CaptureKind::Output {
            device: "default".into(),
        },
        gain: 1.0,
    }];
    for device in screencapturekit::audio_devices::AudioInputDevice::list() {
        sources.push(CaptureSource {
            name: device.name.clone(),
            kind: CaptureKind::Input { device: device.id },
            gain: 1.0,
        });
    }
    if let Ok(content) = SCShareableContent::get() {
        for app in content.applications() {
            let process_id = u32::try_from(app.process_id()).unwrap_or_default();
            if process_id == 0 {
                continue;
            }
            sources.push(CaptureSource {
                name: app.application_name().to_string(),
                kind: CaptureKind::Application {
                    process_id,
                    identity: app.bundle_identifier(),
                    exclude: false,
                },
                gain: 1.0,
            });
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
    let content = SCShareableContent::get().map_err(|error|format!("Allow ApricotPlayer in System Settings > Privacy & Security > Screen & System Audio Recording, then try again. {error}"))?;
    let displays = content.displays();
    let display = displays
        .first()
        .ok_or("No display is available for system audio capture.")?;
    let applications = content.applications();
    let mut builder = SCContentFilter::create().with_display(display);
    let mut selected = Vec::new();
    if let CaptureKind::Application {
        process_id,
        identity,
        exclude,
    } = &source.kind
    {
        let app = applications
            .iter()
            .find(|app| {
                if identity.is_empty() {
                    u32::try_from(app.process_id()).ok() == Some(*process_id)
                } else {
                    app.bundle_identifier() == *identity
                }
            })
            .ok_or(
                "The selected application is no longer running. Choose it again in Recording.",
            )?;
        selected.push(app);
        builder = if *exclude {
            builder.with_excluding_applications(&selected, &[])
        } else {
            builder.with_including_applications(&selected, &[])
        };
    } else {
        builder = builder.with_excluding_windows(&[])
    }
    let filter = builder.build().map_err(|error| error.to_string())?;
    let microphone = matches!(source.kind, CaptureKind::Input { .. });
    let mut config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(!microphone)
        .with_excludes_current_process_audio(false)
        .with_sample_rate(CAPTURE_RATE as i32)
        .with_channel_count(2);
    if let CaptureKind::Input { device } = &source.kind {
        config
            .set_captures_microphone(true)
            .map_err(|error| error.to_string())?;
        if !device.is_empty() {
            config
                .set_microphone_capture_device_id(device)
                .map_err(|error| error.to_string())?;
        }
    }
    let mut stream = SCStream::new(&filter, &config).map_err(|error| error.to_string())?;
    let output_type = if microphone {
        SCStreamOutputType::Microphone
    } else {
        SCStreamOutputType::Audio
    };
    let resampler = Mutex::new(Resampler::default());
    let capture_queue = Arc::clone(&queue);
    stream
        .add_output_handler(
            move |sample: CMSampleBuffer, _: SCStreamOutputType| {
                let Some(format) = sample.format_description() else {
                    return;
                };
                let rate = format
                    .audio_sample_rate()
                    .unwrap_or(f64::from(CAPTURE_RATE));
                let channels = format.audio_channel_count().unwrap_or(2) as usize;
                if format.audio_bits_per_channel() != Some(32)
                    || format
                        .audio_format_flags()
                        .is_some_and(|flags| flags & 1 == 0)
                {
                    capture_queue
                        .fail("The selected source returned an unsupported audio sample format.");
                    return;
                }
                let Ok(buffers) = sample.audio_buffer_list() else {
                    return;
                };
                let planes: Vec<_> = (&buffers)
                    .into_iter()
                    .map(|buffer| {
                        (
                            buffer.number_channels as usize,
                            buffer
                                .data()
                                .chunks_exact(4)
                                .map(|bytes| {
                                    f32::from_le_bytes(bytes.try_into().unwrap_or_default())
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                if planes.is_empty() || channels == 0 {
                    return;
                }
                let frames = if planes.len() == 1 {
                    planes[0].1.len() / planes[0].0.max(1)
                } else {
                    planes
                        .iter()
                        .map(|plane| plane.1.len())
                        .min()
                        .unwrap_or_default()
                };
                let mut stereo = Vec::with_capacity(frames * 2);
                for frame in 0..frames {
                    let (left, right) = if planes.len() == 1 {
                        let n = planes[0].0.max(1);
                        let left = planes[0].1[frame * n];
                        (
                            left,
                            if n > 1 {
                                planes[0].1[frame * n + 1]
                            } else {
                                left
                            },
                        )
                    } else {
                        (
                            planes[0].1[frame],
                            planes
                                .get(1)
                                .map_or(planes[0].1[frame], |plane| plane.1[frame]),
                        )
                    };
                    stereo.extend([left, right]);
                }
                if let Ok(mut resampler) = resampler.lock() {
                    capture_queue.push(resampler.convert(&stereo, rate));
                }
            },
            output_type,
        )
        .map_err(|error| error.to_string())?;
    stream.start_capture().map_err(|error| {
        format!(
            "Allow recording and microphone access in System Settings > Privacy & Security. {error}"
        )
    })?;
    if ready.send(Ok(())).is_err() {
        let _ = stream.stop_capture();
        return Ok(());
    }
    while !stop.load(Ordering::Acquire) {
        if let Some(error) = queue.take_error() {
            let _ = stream.stop_capture();
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stream.stop_capture().map_err(|error| error.to_string())
}

#[derive(Default)]
struct Resampler {
    phase: f64,
    previous: Option<[f32; 2]>,
}

impl Resampler {
    fn convert(&mut self, samples: &[f32], rate: f64) -> Vec<f32> {
        if !rate.is_finite() || rate <= 0.0 {
            return Vec::new();
        }
        let mut frames: Vec<[f32; 2]> = self
            .previous
            .into_iter()
            .chain(samples.chunks_exact(2).map(|frame| [frame[0], frame[1]]))
            .collect();
        if frames.len() < 2 {
            self.previous = frames.pop();
            return Vec::new();
        }
        let mut output = Vec::new();
        let step = rate / f64::from(CAPTURE_RATE);
        while self.phase < (frames.len() - 1) as f64 {
            let index = self.phase.floor() as usize;
            let fraction = (self.phase - index as f64) as f32;
            output.extend([
                frames[index][0] + (frames[index + 1][0] - frames[index][0]) * fraction,
                frames[index][1] + (frames[index + 1][1] - frames[index][1]) * fraction,
            ]);
            self.phase += step;
        }
        self.phase -= (frames.len() - 1) as f64;
        self.previous = frames.last().copied();
        output
    }
}
