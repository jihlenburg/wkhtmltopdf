// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// c_consumer: Rust integration test that shells out to ctest/run.sh,
// which builds the C consumer against libwkhtmltox.dylib and runs it.
//
// This test is #[ignore]d because it requires Chrome (Chromium) to be
// installed and discoverable on PATH.  Run it explicitly with:
//
//   cargo test -p wkhtmltox-capi -- --ignored
//
// or:
//
//   cargo test -p wkhtmltox-capi c_consumer -- --ignored --nocapture

use std::path::PathBuf;
use std::process::Command;

/// Full C consumer test: build libwkhtmltox → compile consumer.c → run → PASS.
///
/// Requires Chrome.  Run with `cargo test -p wkhtmltox-capi -- --ignored`.
#[test]
#[ignore = "requires Chrome; run explicitly with --ignored"]
fn c_consumer_builds_and_passes() {
    // CARGO_MANIFEST_DIR is the capi crate root at compile time.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let run_sh = manifest_dir.join("ctest").join("run.sh");

    assert!(run_sh.exists(), "run.sh not found at {}", run_sh.display());

    let status = Command::new("bash")
        .arg(&run_sh)
        .status()
        .unwrap_or_else(|e| panic!("failed to execute {}: {}", run_sh.display(), e));

    assert!(
        status.success(),
        "C consumer integration test failed (exit code: {:?})",
        status.code()
    );
}
