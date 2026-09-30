//! The files `build_release.ps1` produced pass the checks the updater runs on
//! a downloaded update. Run with `APRICOT_RELEASE_DIR` set to the release folder.

use std::path::PathBuf;

use apricot_updater::{RUST_BETA_PACKAGE, package::validate_update_package};

#[test]
#[ignore = "needs APRICOT_RELEASE_DIR from build_release.ps1"]
fn built_release_files_pass_the_update_checks() {
    let folder =
        PathBuf::from(std::env::var_os("APRICOT_RELEASE_DIR").expect("APRICOT_RELEASE_DIR"));
    for name in [RUST_BETA_PACKAGE.portable[0], RUST_BETA_PACKAGE.installer] {
        validate_update_package(&RUST_BETA_PACKAGE, &folder.join(name))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}
