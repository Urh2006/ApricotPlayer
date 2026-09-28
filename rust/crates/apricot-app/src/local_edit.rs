//! Python edit mode (E, Ctrl+S, Ctrl+R): bakes the player's speed, pitch and
//! equalizer into a local media file. The argument builders mirror Python
//! `local_edit_mpv_render_args`, `local_edit_audio_filters`,
//! `local_edit_audio_codec_args`, `local_edit_ffmpeg_args` and the mux step of
//! `save_edited_local_file_worker`.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use apricot_playback::{PitchMode, SpeedAudioMode, equalizer_filters, rubberband_pitch_filter};

use crate::equalizer::EqualizerGains;

/// Python `is_video_file_extension`.
const VIDEO_EXTENSIONS: [&str; 11] = [
    "3g2", "3gp", "avi", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "webm", "wmv",
];

/// The player's audio state that edit mode writes into the file.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalEditAudio {
    pub speed: f64,
    pub pitch: f64,
    pub speed_mode: SpeedAudioMode,
    pub pitch_mode: PitchMode,
    /// Python `effective_equalizer_state` gains while the equalizer is on,
    /// including bass boost.
    pub equalizer: Option<EqualizerGains>,
    /// Python `settings.equalizer_clipping_protection`.
    pub clipping_protection: bool,
}

impl LocalEditAudio {
    /// Python `max(0.25, min(4.0, current_speed_value()))`.
    pub fn speed(&self) -> f64 {
        self.speed.clamp(0.25, 4.0)
    }

    /// Python `max(0.5, min(2.0, current_pitch_value()))`.
    pub fn pitch(&self) -> f64 {
        self.pitch.clamp(0.5, 2.0)
    }

    fn equalizer_filters(&self) -> Vec<String> {
        self.equalizer.as_ref().map_or_else(Vec::new, |gains| {
            equalizer_filters(gains, self.clipping_protection)
        })
    }

    /// Python `local_edit_mpv_render_args` without the executable: mpv plays
    /// the file silently into a PCM WAV file with the player's audio chain.
    ///
    /// Python always appends the equalizer while it is on, and mpv refuses the
    /// empty `lavfi=[]` graph of a flat equalizer, so the save fails. Rust
    /// leaves a flat equalizer out, as the player itself does (proposal P-3).
    pub fn mpv_render_arguments(&self, source: &Path, output: &Path) -> Vec<OsString> {
        let pitch = self.pitch();
        let mut arguments = vec![
            source.as_os_str().to_owned(),
            "--no-config".into(),
            format!("--speed={:.6}", self.speed()).into(),
            "--video=no".into(),
            "--sub=no".into(),
            "--ao=pcm".into(),
        ];
        // Python `speed_audio_filter_args`.
        arguments.push(
            format!(
                "--audio-pitch-correction={}",
                if self.speed_mode.audio_pitch_correction() {
                    "yes"
                } else {
                    "no"
                }
            )
            .into(),
        );
        if let Some(filter) = self.speed_mode.filter() {
            arguments.push(format!("--af={filter}").into());
        }
        arguments.push("--audio-pitch-correction=yes".into());
        if self.pitch_mode.uses_mpv_pitch() {
            arguments.push(format!("--pitch={pitch:.6}").into());
        } else {
            arguments.push("--pitch=1.000000".into());
            if (pitch - 1.0).abs() >= 0.001 {
                arguments.push(format!("--af-append={}", rubberband_pitch_filter(pitch)).into());
            }
        }
        let equalizer = self.equalizer_filters();
        if !equalizer.is_empty() {
            arguments.push(
                format!(
                    "--af-append=@{}:lavfi=[{}]",
                    apricot_playback::EQUALIZER_FILTER_LABEL,
                    equalizer.join(",")
                )
                .into(),
            );
        }
        let mut pcm_file = OsString::from("--ao-pcm-file=");
        pcm_file.push(output.as_os_str());
        arguments.push(pcm_file);
        arguments
    }

    /// Python `local_edit_audio_filters`: the equalizer, then Rubberband for a
    /// pitch change or an `atempo` chain for a speed change alone.
    pub fn ffmpeg_audio_filters(&self) -> Vec<String> {
        let speed = self.speed();
        let pitch = self.pitch();
        let mut filters = self.equalizer_filters();
        let has_pitch = (pitch - 1.0).abs() >= 0.001;
        let has_speed = (speed - 1.0).abs() >= 0.001;
        if has_pitch && has_speed {
            filters.push(format!(
                "rubberband=pitch={pitch:.6}:tempo={speed:.6}:phase=independent:pitchq=quality"
            ));
        } else if has_pitch {
            filters.push(format!(
                "rubberband=pitch={pitch:.6}:phase=independent:pitchq=quality"
            ));
        } else if has_speed {
            filters.extend(ffmpeg_atempo_chain(speed));
        }
        filters
    }

    /// The mux step of Python `save_edited_local_file_worker` without the
    /// executable: the rendered PCM becomes the audio of the output file.
    pub fn mux_arguments(&self, source: &Path, pcm: &Path, output: &Path) -> Vec<OsString> {
        let speed = self.speed();
        let mut arguments = ffmpeg_prefix();
        if is_video_file(source) {
            arguments.extend(["-i".into(), source.as_os_str().to_owned()]);
            arguments.extend(["-i".into(), pcm.as_os_str().to_owned()]);
            if (speed - 1.0).abs() >= 0.001 {
                arguments.extend(["-vf".into(), video_speed_filter(speed).into()]);
                arguments.extend(os_strings(&["-map", "0:v:0", "-map", "1:a:0"]));
                arguments.extend(os_strings(&[
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "18",
                ]));
            } else {
                arguments.extend(os_strings(&[
                    "-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy",
                ]));
            }
            arguments.extend(os_strings(&audio_codec_arguments(output)));
            arguments.push("-shortest".into());
        } else {
            arguments.extend(["-i".into(), pcm.as_os_str().to_owned()]);
            arguments.extend(os_strings(&audio_codec_arguments(output)));
        }
        arguments.push(output.as_os_str().to_owned());
        arguments
    }

    /// Python `local_edit_ffmpeg_args` without the executable, used when mpv
    /// is not available.
    pub fn ffmpeg_arguments(&self, source: &Path, output: &Path) -> Vec<OsString> {
        let speed = self.speed();
        let audio_filters = self.ffmpeg_audio_filters();
        let mut arguments = ffmpeg_prefix();
        arguments.extend(["-i".into(), source.as_os_str().to_owned()]);
        if audio_filters.is_empty() {
            arguments.extend(os_strings(&["-c:a", "copy"]));
        } else {
            arguments.extend(["-af".into(), audio_filters.join(",").into()]);
            arguments.extend(os_strings(&audio_codec_arguments(output)));
        }
        if is_video_file(source) {
            if (speed - 1.0).abs() >= 0.001 {
                arguments.extend(["-vf".into(), video_speed_filter(speed).into()]);
                arguments.extend(os_strings(&[
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "18",
                ]));
            } else {
                arguments.extend(os_strings(&["-c:v", "copy"]));
            }
        }
        arguments.push(output.as_os_str().to_owned());
        arguments
    }
}

/// Python `ffmpeg_atempo_chain`: `atempo` accepts 0.5 to 2.0 per stage.
pub fn ffmpeg_atempo_chain(factor: f64) -> Vec<String> {
    let mut values = Vec::new();
    let mut factor = if factor.is_finite() && factor != 0.0 {
        factor
    } else {
        1.0
    }
    .clamp(0.0625, 16.0);
    while factor < 0.5 {
        values.push("atempo=0.5".to_owned());
        factor /= 0.5;
    }
    while factor > 2.0 {
        values.push("atempo=2.0".to_owned());
        factor /= 2.0;
    }
    if (factor - 1.0).abs() >= 0.001 {
        values.push(format!("atempo={factor:.6}"));
    }
    values
}

/// Python `local_edit_audio_codec_args` for the output file's extension.
pub fn audio_codec_arguments(output: &Path) -> Vec<&'static str> {
    match extension(output).as_str() {
        "mp3" => vec!["-c:a", "libmp3lame", "-b:a", "320k"],
        "opus" => vec!["-c:a", "libopus", "-b:a", "160k"],
        "wav" => vec!["-c:a", "pcm_s16le"],
        "flac" => vec!["-c:a", "flac"],
        _ => vec!["-c:a", "aac", "-b:a", "256k"],
    }
}

/// Python `is_video_file_extension`.
pub fn is_video_file(path: &Path) -> bool {
    VIDEO_EXTENSIONS.contains(&extension(path).as_str())
}

/// Python `edited_output_path`: "name - edited.ext", then "name - edited (2).ext"
/// and so on, or the source itself when it is replaced.
pub fn edited_output_path(source: &Path, replace_original: bool) -> PathBuf {
    if replace_original {
        return source.to_path_buf();
    }
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = source
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut output = source.with_file_name(format!("{stem} - edited{suffix}"));
    let mut counter = 2;
    while output.exists() {
        output = source.with_file_name(format!("{stem} - edited ({counter}){suffix}"));
        counter += 1;
    }
    output
}

/// Python `temporary_conversion_path`: a hidden sibling that replaces the
/// original only after the edit succeeded.
pub fn temporary_conversion_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let token = rand::random::<[u8; 6]>()
        .iter()
        .fold(String::new(), |mut token, byte| {
            use std::fmt::Write as _;
            let _ = write!(token, "{byte:02x}");
            token
        });
    path.with_file_name(format!(".{stem}.apricot-converting-{token}{suffix}"))
}

/// Python `temp_output.with_suffix(".render_NNNN.wav")`.
pub fn pcm_render_path(output: &Path) -> PathBuf {
    let number: u16 = rand::random_range(1000..=9999);
    output.with_extension(format!("render_{number}.wav"))
}

fn video_speed_filter(speed: f64) -> String {
    format!("setpts={:.8}*PTS", 1.0 / speed)
}

fn ffmpeg_prefix() -> Vec<OsString> {
    os_strings(&["-y", "-hide_banner", "-loglevel", "error"])
}

fn os_strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
    };

    use apricot_playback::{PitchMode, SpeedAudioMode};

    use super::{
        LocalEditAudio, audio_codec_arguments, edited_output_path, ffmpeg_atempo_chain,
        is_video_file, pcm_render_path, temporary_conversion_path,
    };

    fn audio(speed: f64, pitch: f64) -> LocalEditAudio {
        LocalEditAudio {
            speed,
            pitch,
            speed_mode: SpeedAudioMode::from_setting("Rubberband high quality"),
            pitch_mode: PitchMode::from_setting("Independent pitch - advanced (Rubberband)"),
            equalizer: None,
            clipping_protection: false,
        }
    }

    fn strings(arguments: &[OsString]) -> Vec<String> {
        arguments
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn atempo_chain_splits_like_python() {
        assert_eq!(ffmpeg_atempo_chain(1.0), Vec::<String>::new());
        assert_eq!(ffmpeg_atempo_chain(1.5), ["atempo=1.500000"]);
        assert_eq!(ffmpeg_atempo_chain(3.0), ["atempo=2.0", "atempo=1.500000"]);
        assert_eq!(ffmpeg_atempo_chain(4.0), ["atempo=2.0", "atempo=2.000000"]);
        assert_eq!(ffmpeg_atempo_chain(0.25), ["atempo=0.5", "atempo=0.500000"]);
        assert_eq!(ffmpeg_atempo_chain(0.3), ["atempo=0.5", "atempo=0.600000"]);
        assert_eq!(ffmpeg_atempo_chain(0.0), Vec::<String>::new());
    }

    #[test]
    fn mpv_render_matches_python_for_rubberband_pitch_and_equalizer() {
        let mut edit = audio(1.5, 1.2);
        edit.equalizer = Some(crate::equalizer::factory_gains("bass_boost"));
        let arguments = strings(
            &edit.mpv_render_arguments(Path::new(r"C:\m\song.mka"), Path::new(r"C:\m\out.wav")),
        );
        assert_eq!(
            arguments[..9],
            [
                r"C:\m\song.mka",
                "--no-config",
                "--speed=1.500000",
                "--video=no",
                "--sub=no",
                "--ao=pcm",
                "--audio-pitch-correction=yes",
                "--af=@apricot_speed:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer",
                "--audio-pitch-correction=yes",
            ]
        );
        assert_eq!(arguments[9], "--pitch=1.000000");
        assert_eq!(
            arguments[10],
            "--af-append=@apricot_pitch:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale=1.2000"
        );
        assert!(
            arguments[11].starts_with("--af-append=@apricot_eq:lavfi=[equalizer=f=31:"),
            "{}",
            arguments[11]
        );
        assert_eq!(arguments[12], r"--ao-pcm-file=C:\m\out.wav");
        assert_eq!(arguments.len(), 13);
    }

    #[test]
    fn mpv_render_uses_mpv_pitch_and_skips_a_flat_equalizer() {
        let mut edit = audio(1.0, 0.9);
        edit.speed_mode = SpeedAudioMode::from_setting("mpv default scaletempo2");
        edit.pitch_mode = PitchMode::from_setting("mpv pitch");
        edit.equalizer = Some(crate::equalizer::EqualizerGains::new());
        let arguments =
            strings(&edit.mpv_render_arguments(Path::new("a.mp3"), Path::new("a.render_1.wav")));
        assert_eq!(
            arguments,
            [
                "a.mp3",
                "--no-config",
                "--speed=1.000000",
                "--video=no",
                "--sub=no",
                "--ao=pcm",
                "--audio-pitch-correction=yes",
                "--audio-pitch-correction=yes",
                "--pitch=0.900000",
                "--ao-pcm-file=a.render_1.wav",
            ]
        );
    }

    #[test]
    fn ffmpeg_filters_match_python_local_edit_audio_filters() {
        assert_eq!(
            audio(1.25, 1.1).ffmpeg_audio_filters(),
            ["rubberband=pitch=1.100000:tempo=1.250000:phase=independent:pitchq=quality"]
        );
        assert_eq!(
            audio(1.0, 0.8).ffmpeg_audio_filters(),
            ["rubberband=pitch=0.800000:phase=independent:pitchq=quality"]
        );
        assert_eq!(
            audio(8.0, 1.0).ffmpeg_audio_filters(),
            ["atempo=2.0", "atempo=2.000000"]
        );
        let mut equalized = audio(1.0, 1.0);
        equalized.equalizer = Some(crate::equalizer::EqualizerGains::from([(
            "1000".to_owned(),
            -3.0,
        )]));
        assert_eq!(
            equalized.ffmpeg_audio_filters(),
            ["equalizer=f=1000:t=q:w=2:g=-3.0"]
        );
    }

    #[test]
    fn ffmpeg_fallback_copies_streams_without_changes() {
        let arguments = strings(
            &audio(1.0, 1.0)
                .ffmpeg_arguments(Path::new("clip.mp4"), Path::new("clip - edited.mp4")),
        );
        assert_eq!(
            arguments,
            [
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                "clip.mp4",
                "-c:a",
                "copy",
                "-c:v",
                "copy",
                "clip - edited.mp4",
            ]
        );
        let arguments = strings(
            &audio(2.0, 1.0)
                .ffmpeg_arguments(Path::new("clip.mp4"), Path::new("clip - edited.mp4")),
        );
        assert_eq!(
            arguments,
            [
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                "clip.mp4",
                "-af",
                "atempo=2.000000",
                "-c:a",
                "aac",
                "-b:a",
                "256k",
                "-vf",
                "setpts=0.50000000*PTS",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "18",
                "clip - edited.mp4",
            ]
        );
    }

    #[test]
    fn mux_matches_python_for_audio_and_video() {
        let edit = audio(2.0, 1.0);
        assert_eq!(
            strings(&edit.mux_arguments(
                Path::new("song.flac"),
                Path::new("p.wav"),
                Path::new("song - edited.flac")
            )),
            [
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                "p.wav",
                "-c:a",
                "flac",
                "song - edited.flac",
            ]
        );
        assert_eq!(
            strings(&edit.mux_arguments(
                Path::new("film.mkv"),
                Path::new("p.wav"),
                Path::new("film - edited.mkv")
            )),
            [
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                "film.mkv",
                "-i",
                "p.wav",
                "-vf",
                "setpts=0.50000000*PTS",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "18",
                "-c:a",
                "aac",
                "-b:a",
                "256k",
                "-shortest",
                "film - edited.mkv",
            ]
        );
        assert_eq!(
            strings(&audio(1.0, 1.1).mux_arguments(
                Path::new("film.MOV"),
                Path::new("p.wav"),
                Path::new("film - edited.MOV")
            ))[8..14],
            ["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy"]
        );
    }

    #[test]
    fn codecs_and_video_extensions_match_python() {
        assert_eq!(
            audio_codec_arguments(Path::new("a.MP3")),
            ["-c:a", "libmp3lame", "-b:a", "320k"]
        );
        assert_eq!(
            audio_codec_arguments(Path::new("a.m4a")),
            ["-c:a", "aac", "-b:a", "256k"]
        );
        assert_eq!(
            audio_codec_arguments(Path::new("a.opus")),
            ["-c:a", "libopus", "-b:a", "160k"]
        );
        assert_eq!(
            audio_codec_arguments(Path::new("a.wav")),
            ["-c:a", "pcm_s16le"]
        );
        assert_eq!(audio_codec_arguments(Path::new("a.flac")), ["-c:a", "flac"]);
        assert_eq!(
            audio_codec_arguments(Path::new("a.ogg")),
            ["-c:a", "aac", "-b:a", "256k"]
        );
        assert!(is_video_file(Path::new("a.WEBM")));
        assert!(!is_video_file(Path::new("a.mka")));
    }

    #[test]
    fn output_paths_match_python() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("song.mp3");
        std::fs::write(&source, b"x").expect("source");
        assert_eq!(edited_output_path(&source, true), source);
        let first = edited_output_path(&source, false);
        assert_eq!(first, folder.path().join("song - edited.mp3"));
        std::fs::write(&first, b"x").expect("first copy");
        assert_eq!(
            edited_output_path(&source, false),
            folder.path().join("song - edited (2).mp3")
        );
        let temporary = temporary_conversion_path(&source);
        let name = temporary
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with(".song.apricot-converting-"), "{name}");
        assert_eq!(
            temporary.extension().and_then(|value| value.to_str()),
            Some("mp3")
        );
        assert_eq!(name.len(), ".song.apricot-converting-".len() + 12 + 4);
        let pcm = pcm_render_path(&PathBuf::from(r"C:\m\song - edited.mp3"));
        let pcm = pcm.to_string_lossy();
        assert!(pcm.starts_with(r"C:\m\song - edited.render_"), "{pcm}");
        assert!(pcm.ends_with(".wav"), "{pcm}");
    }
}
