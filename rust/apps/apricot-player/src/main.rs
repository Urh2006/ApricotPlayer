#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use apricot_app::{ActivationRequest, Application, MainMenuAvailability, SettingsController};
use apricot_core::{
    action::ACTIONS, locale::LANGUAGES, menu::CUSTOMIZABLE_MAIN_MENU, setting::SettingId,
};
use apricot_media::{YoutubeCommand, YoutubeEngine, YoutubeResponsePayload};
use apricot_platform::{
    ApplicationIdentity, SingleInstanceOutcome, YoutubeHelperProcess, YtDlpYoutubeEngine,
    acquire_single_instance, discover_windows_beta_paths, sync_startup_registration,
};
use apricot_storage::{
    MediaListFile, PlaybackQueueFile, SettingsDocument, SettingsPaths, UserPlaylistFile,
};
use apricot_updater::UpdateChannel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let start_hidden = arguments
        .iter()
        .any(|argument| argument == "--start-in-tray");
    if run_qualification(&arguments)? {
        return Ok(());
    }

    let paths = discover_windows_beta_paths()?;
    let defaults = SettingsDocument::with_platform_defaults(
        paths.downloads.to_string_lossy().into_owned(),
        paths.cache.to_string_lossy().into_owned(),
        "beta",
    );
    let settings_paths = SettingsPaths::for_app_data(&paths.app_data, &paths.legacy_app_data);
    let settings = SettingsController::load(settings_paths, defaults);
    let startup_file = startup_file_argument(&arguments);
    let instance = acquire_single_instance(ApplicationIdentity::RustBeta)?;
    let _instance_guard = match instance {
        SingleInstanceOutcome::Primary(guard) => guard,
        SingleInstanceOutcome::Secondary => {
            if let Some(path) = startup_file {
                apricot_ui_windows::forward_to_existing(&ActivationRequest::OpenFile(path))?;
            } else if settings.current().close_to_tray {
                apricot_ui_windows::forward_to_existing(&ActivationRequest::Show)?;
            } else {
                let catalog = apricot_app::embedded_catalog(&settings.current().language);
                let message = catalog.text("already_open");
                apricot_ui_windows::show_already_open(message);
            }
            return Ok(());
        }
    };
    if let Ok(executable) = std::env::current_exe() {
        let _ = sync_startup_registration(
            ApplicationIdentity::RustBeta,
            &executable,
            settings.current().start_with_windows,
        );
    }
    let mut application = Application::new(settings, MainMenuAvailability::default());
    let legacy_queue = PlaybackQueueFile::new(paths.legacy_app_data.join("playback_queue.json"));
    application.configure_playback_queue(
        PlaybackQueueFile::new(paths.app_data.join("playback_queue.json")),
        &legacy_queue,
    );
    application.configure_media_collections(
        MediaListFile::new(paths.app_data.join("favorites.json")),
        &MediaListFile::new(paths.legacy_app_data.join("favorites.json")),
        MediaListFile::new(paths.app_data.join("history.json")),
        &MediaListFile::new(paths.legacy_app_data.join("history.json")),
    );
    application.configure_user_playlists(
        UserPlaylistFile::new(paths.app_data.join("playlists.json")),
        &UserPlaylistFile::new(paths.legacy_app_data.join("playlists.json")),
    );
    if !application.settings().language_prompted {
        let selected =
            apricot_ui_windows::choose_initial_language(&application.settings().language)?;
        application.complete_initial_language(selected)?;
    }
    if let Some(path) = startup_file {
        application.enqueue_activation(ActivationRequest::OpenFile(path));
    }
    apricot_ui_windows::run_application(application, env!("CARGO_PKG_VERSION"), start_hidden)
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

fn run_qualification(arguments: &[std::ffi::OsString]) -> Result<bool, Box<dyn std::error::Error>> {
    if arguments
        .iter()
        .any(|argument| argument == "--qualification-smoke")
    {
        assert_eq!(SettingId::ALL.len(), 117);
        assert_eq!(ACTIONS.len(), 91);
        assert_eq!(CUSTOMIZABLE_MAIN_MENU.len(), 19);
        assert_eq!(LANGUAGES.len(), 27);
        assert!(!UpdateChannel::LocalOnly.allows_remote_install());
        return Ok(true);
    }
    if arguments
        .iter()
        .any(|argument| argument == "--qualification-youtube-helper")
    {
        let executable = std::env::current_exe()?;
        let helper_path = executable
            .parent()
            .ok_or("application executable has no parent directory")?
            .join("components")
            .join("apricot-youtube-helper.exe");
        let helper = YoutubeHelperProcess::start(&helper_path)?;
        assert!(!helper.helper_version().is_empty());
        assert!(!helper.backend_revision().is_empty());
        return Ok(true);
    }
    if arguments
        .iter()
        .any(|argument| argument == "--qualification-ytdlp")
    {
        let executable = std::env::current_exe()?;
        let ytdlp_path = executable
            .parent()
            .ok_or("application executable has no parent directory")?
            .join("components")
            .join("yt-dlp.exe");
        let mut engine = YtDlpYoutubeEngine::new(&ytdlp_path)?;
        let response = engine.execute(YoutubeCommand::Hello)?;
        assert!(matches!(
            response,
            YoutubeResponsePayload::Hello {
                helper_version,
                backend_revision,
                ..
            } if !helper_version.is_empty() && helper_version == backend_revision
        ));
        return Ok(true);
    }
    if arguments
        .iter()
        .any(|argument| argument == "--qualification-playback")
    {
        qualify_packaged_playback()?;
        return Ok(true);
    }
    Ok(false)
}

fn qualify_packaged_playback() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        collections::BTreeMap,
        fs,
        time::{Duration, Instant},
    };

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_playback::{
        MpvLaunchOptions, MpvVideoMode, PlaybackCommand, PlaybackEvent, PlaybackRuntime,
    };

    let executable = std::env::current_exe()?;
    let root = executable
        .parent()
        .ok_or("application executable has no parent directory")?;
    let fixture = std::env::temp_dir().join(format!(
        "apricot-player-qualification-{}.wav",
        std::process::id()
    ));
    write_silent_wav(&fixture)?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut options = MpvLaunchOptions::new(root.join("mpv").join("mpv.exe"));
        options.library = Some(root.join("mpv").join("libmpv-2.dll"));
        options.video_mode = MpvVideoMode::AudioOnly;
        options.audio_driver = Some("null".to_owned());
        let item = MediaItem {
            id: MediaId("qualification".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Qualification".to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(fixture.to_string_lossy().into_owned()),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        };
        let runtime = PlaybackRuntime::spawn()?;
        runtime.start(1, options, item)?;
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if let Some(update) = runtime.poll_update()? {
                match update.event {
                    PlaybackEvent::Started => break,
                    PlaybackEvent::Failed(error) => return Err(error.into()),
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                return Err("packaged libmpv playback timed out".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        runtime.execute(1, PlaybackCommand::SetVolume(42.0))?;
        runtime.execute(
            1,
            PlaybackCommand::SeekAbsolute {
                seconds: 0.05,
                exact: true,
            },
        )?;
        runtime.close(1)?;
        Ok(())
    })();
    let _ = fs::remove_file(&fixture);
    result
}

fn write_silent_wav(path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;

    let sample_rate = 44_100_u32;
    let channels = 2_u16;
    let bits_per_sample = 16_u16;
    let frames = sample_rate / 4;
    let data_size = frames * u32::from(channels) * u32::from(bits_per_sample / 8);
    let mut file = std::fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample / 8);
    file.write_all(&byte_rate.to_le_bytes())?;
    let block_align = channels * (bits_per_sample / 8);
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;
    file.write_all(&vec![
        0_u8;
        usize::try_from(data_size)
            .expect("qualification WAV size fits usize")
    ])?;
    Ok(())
}

fn startup_file_argument(arguments: &[std::ffi::OsString]) -> Option<PathBuf> {
    arguments
        .iter()
        .find(|argument| !argument.to_string_lossy().starts_with("--"))
        .map(PathBuf::from)
}
