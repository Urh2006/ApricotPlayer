use std::{env, fs, path::PathBuf};

use apricot_core::setting::SettingId;
use apricot_storage::SettingsDocument;
use serde_json::Value;

#[test]
#[ignore = "run through qualify_python_settings_compat.ps1 with a private temporary copy"]
fn real_python_settings_round_trip_without_key_loss() {
    let path = PathBuf::from(
        env::var_os("APRICOT_PYTHON_SETTINGS")
            .expect("APRICOT_PYTHON_SETTINGS must point to a private temporary copy"),
    );
    let input: Value =
        serde_json::from_slice(&fs::read(&path).expect("read copied settings")).expect("JSON");
    let input_object = input.as_object().expect("settings object");
    let settings = SettingsDocument::from_value_with_defaults(
        &input,
        SettingsDocument::with_platform_defaults(
            input_object
                .get("download_folder")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            input_object
                .get("cache_folder")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            input_object
                .get("update_channel")
                .and_then(Value::as_str)
                .unwrap_or("stable"),
        ),
    )
    .expect("Rust settings import");
    let output = serde_json::to_value(&settings).expect("serialize");
    let output_object = output.as_object().expect("output object");
    for (key, value) in input_object {
        assert!(output_object.contains_key(key), "lost key {key}");
        assert_eq!(&output_object[key], value, "changed imported key {key}");
    }
    for id in SettingId::ALL {
        assert!(output_object.contains_key(id.key()), "missing {}", id.key());
    }

    let second: SettingsDocument =
        serde_json::from_value(output.clone()).expect("second deserialize");
    assert_eq!(
        serde_json::to_value(second).expect("second serialize"),
        output
    );
}
