// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// image_consumer: Rust integration test that shells out to ctest/run_image.sh,
// which builds the C image consumer against libwkhtmltox.dylib and runs it.
//
// This test is #[ignore]d because it requires Chrome (Chromium) to be
// installed and discoverable on PATH.  Run it explicitly with:
//
//   cargo test -p wkhtmltox-capi -- --ignored
//
// or:
//
//   cargo test -p wkhtmltox-capi image_consumer -- --ignored --nocapture

use std::path::PathBuf;
use std::process::Command;

/// Full C image consumer test: build libwkhtmltox → compile image_consumer.c →
/// run → output is a valid PNG.
///
/// Requires Chrome.  Run with `cargo test -p wkhtmltox-capi -- --ignored`.
#[test]
#[ignore = "requires Chrome; run explicitly with --ignored"]
fn image_c_consumer_builds_and_passes() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let run_sh = manifest_dir.join("ctest").join("run_image.sh");

    assert!(
        run_sh.exists(),
        "run_image.sh not found at {}",
        run_sh.display()
    );

    let status = Command::new("bash")
        .arg(&run_sh)
        .status()
        .unwrap_or_else(|e| panic!("failed to execute {}: {}", run_sh.display(), e));

    assert!(
        status.success(),
        "C image consumer integration test failed (exit code: {:?})",
        status.code()
    );
}
