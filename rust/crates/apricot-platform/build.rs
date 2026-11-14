//! The `screencapturekit` bridge links the Swift compatibility libraries from
//! the Xcode toolchain layout only. When `xcode-select` points at the Command
//! Line Tools, those libraries live under `usr/lib/swift/macosx` instead.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let Ok(output) = Command::new("xcode-select").arg("-p").output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let developer = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let swift = Path::new(&developer).join("usr/lib/swift/macosx");
    if swift.join("libswiftCompatibility56.a").is_file() {
        println!("cargo:rustc-link-search=native={}", swift.display());
    }
}
