use std::{collections::BTreeMap, env, path::PathBuf};

use apricot_storage::{CompatibilitySnapshot, DEFAULT_MAX_ARTIFACT_BYTES};
use tempfile::tempdir;

#[test]
#[ignore = "run through qualify_python_data_compat.ps1 with a private temporary copy"]
fn real_python_data_round_trips_without_value_loss() {
    let source = PathBuf::from(
        env::var_os("APRICOT_PYTHON_APP_DATA")
            .expect("APRICOT_PYTHON_APP_DATA must point to a private temporary copy"),
    );
    let first = CompatibilitySnapshot::load(&source, DEFAULT_MAX_ARTIFACT_BYTES)
        .expect("load Python app data");
    assert!(!first.artifacts.is_empty(), "no compatibility data found");

    let target = tempdir().expect("round-trip directory");
    first.write_to(target.path()).expect("write Rust snapshot");
    let second = CompatibilitySnapshot::load(target.path(), DEFAULT_MAX_ARTIFACT_BYTES)
        .expect("reload Rust snapshot");

    let first_values: BTreeMap<_, _> = first
        .artifacts
        .iter()
        .map(|artifact| (artifact.id, &artifact.value))
        .collect();
    let second_values: BTreeMap<_, _> = second
        .artifacts
        .iter()
        .map(|artifact| (artifact.id, &artifact.value))
        .collect();
    assert_eq!(first_values, second_values);
}
