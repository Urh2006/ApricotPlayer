//! Audio output device choices from Python `show_output_devices`,
//! `audio_output_device_options` and `check_saved_audio_device_available`.

use apricot_playback::AudioOutputDevice;

use crate::SettingsChoiceOption;

pub const AUTO_DEVICE: &str = "auto";

/// Python `normalized_audio_output_device`: blank means `auto`.
#[must_use]
pub fn normalized_device(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        AUTO_DEVICE.to_owned()
    } else {
        value.to_owned()
    }
}

/// Python `show_output_devices`: every reported device, `auto` included,
/// labeled "description (name)" unless both are the same.
#[must_use]
pub fn player_device_choices(devices: &[AudioOutputDevice]) -> Vec<SettingsChoiceOption> {
    devices
        .iter()
        .filter(|device| !device.name.trim().is_empty())
        .map(|device| SettingsChoiceOption::labeled(&device.name, device_label(device)))
        .collect()
}

/// Python `audio_output_device_options(allow_probe=False)`: `auto` and the
/// saved device, so opening Settings never drops a saved device.
#[must_use]
pub fn unprobed_device_options(saved: &str) -> Vec<SettingsChoiceOption> {
    let saved = normalized_device(saved);
    let mut options = vec![SettingsChoiceOption::raw(AUTO_DEVICE)];
    if !saved.eq_ignore_ascii_case(AUTO_DEVICE) {
        options.push(SettingsChoiceOption::raw(saved));
    }
    options
}

/// Python `audio_output_device_options(allow_probe=True)`: `auto` first, then
/// each probed device, then the saved device marked as missing when the probe
/// did not report it.
#[must_use]
pub fn probed_device_options(
    devices: &[AudioOutputDevice],
    saved: &str,
    no_devices_text: &str,
) -> Vec<SettingsChoiceOption> {
    let mut options = vec![SettingsChoiceOption::raw(AUTO_DEVICE)];
    for device in devices {
        if !device.name.trim().is_empty()
            && !options.iter().any(|option| option.value == device.name)
        {
            options.push(SettingsChoiceOption::labeled(
                &device.name,
                device_label(device),
            ));
        }
    }
    let saved = normalized_device(saved);
    if !options.iter().any(|option| option.value == saved) {
        options.push(SettingsChoiceOption::labeled(
            &saved,
            missing_label(&saved, no_devices_text),
        ));
    }
    options
}

/// Python `check_saved_audio_device_available`: true when the saved device
/// is not `auto` and the probe did not report it.
#[must_use]
pub fn saved_device_missing(
    saved: &str,
    options: &[SettingsChoiceOption],
    no_devices_text: &str,
) -> bool {
    let saved = normalized_device(saved);
    if saved.eq_ignore_ascii_case(AUTO_DEVICE) {
        return false;
    }
    !options.iter().any(|option| {
        option.value == saved && option.label != missing_label(&saved, no_devices_text)
    })
}

/// Python `finish_audio_output_device_refresh`: the refreshed list keeps the
/// value the user has selected, even when the probe no longer reports it.
#[must_use]
pub fn refreshed_options_keeping(
    mut options: Vec<SettingsChoiceOption>,
    selected: &str,
) -> Vec<SettingsChoiceOption> {
    if !selected.is_empty() && !options.iter().any(|option| option.value == selected) {
        options.push(SettingsChoiceOption::raw(selected));
    }
    options
}

fn device_label(device: &AudioOutputDevice) -> String {
    if device.description.is_empty() || device.description == device.name {
        device.name.clone()
    } else {
        format!("{} ({})", device.description, device.name)
    }
}

fn missing_label(saved: &str, no_devices_text: &str) -> String {
    format!("{saved} ({no_devices_text})")
}

#[cfg(test)]
mod tests {
    use apricot_playback::AudioOutputDevice;

    use super::*;

    const MISSING: &str = "No audio output devices were found.";

    fn device(name: &str, description: &str) -> AudioOutputDevice {
        AudioOutputDevice {
            name: name.to_owned(),
            description: description.to_owned(),
        }
    }

    fn pairs(options: &[SettingsChoiceOption]) -> Vec<(&str, &str)> {
        options
            .iter()
            .map(|option| (option.value.as_str(), option.label.as_str()))
            .collect()
    }

    #[test]
    fn player_choices_list_every_device_with_python_labels() {
        let devices = [
            device("auto", "Autoselect device"),
            device("wasapi/{1}", "Speakers"),
            device("openal", "openal"),
        ];
        assert_eq!(
            pairs(&player_device_choices(&devices)),
            [
                ("auto", "Autoselect device (auto)"),
                ("wasapi/{1}", "Speakers (wasapi/{1})"),
                ("openal", "openal"),
            ]
        );
    }

    #[test]
    fn settings_keep_the_saved_device_before_the_probe() {
        assert_eq!(pairs(&unprobed_device_options("")), [("auto", "auto")]);
        assert_eq!(
            pairs(&unprobed_device_options("wasapi/{1}")),
            [("auto", "auto"), ("wasapi/{1}", "wasapi/{1}")]
        );
    }

    #[test]
    fn probed_options_put_auto_first_and_mark_a_missing_saved_device() {
        let devices = [
            device("auto", "Autoselect device"),
            device("wasapi/{1}", "Speakers"),
        ];
        let options = probed_device_options(&devices, "wasapi/{2}", MISSING);
        assert_eq!(
            pairs(&options),
            [
                ("auto", "auto"),
                ("wasapi/{1}", "Speakers (wasapi/{1})"),
                (
                    "wasapi/{2}",
                    "wasapi/{2} (No audio output devices were found.)"
                ),
            ]
        );
        assert!(saved_device_missing("wasapi/{2}", &options, MISSING));
        assert!(!saved_device_missing("wasapi/{1}", &options, MISSING));
        assert!(!saved_device_missing("auto", &options, MISSING));
        assert!(!saved_device_missing("", &options, MISSING));
    }

    #[test]
    fn refresh_keeps_the_selected_value() {
        let options = probed_device_options(&[], "auto", MISSING);
        assert_eq!(
            pairs(&refreshed_options_keeping(options.clone(), "wasapi/{3}")),
            [("auto", "auto"), ("wasapi/{3}", "wasapi/{3}")]
        );
        assert_eq!(
            pairs(&refreshed_options_keeping(options, "auto")),
            [("auto", "auto")]
        );
    }
}
