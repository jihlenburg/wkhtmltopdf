// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Exit-code parity tests. These spawn the built binary; the missing-input
//! and bad-flag cases return BEFORE any Chrome spawn, so they need no browser.
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wkhtmltoimage"))
}

#[test]
fn missing_local_input_is_nonzero() {
    // A local path that does not exist must fail (oracle returns nonzero).
    // Assert on the existence-guard's stderr message — not merely nonzero —
    // so the test pins the guard path specifically and cannot pass for the
    // wrong reason (e.g. a Chrome-spawn failure in a browserless environment).
    let out = bin()
        .args(["/no/such/input/file_xyz.html", "/tmp/out_should_not_exist.png"])
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "missing input must be nonzero, got {:?}", out.status);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("input file not found"),
        "expected existence-guard message, got stderr: {stderr}"
    );
}

#[test]
fn unknown_flag_is_nonzero() {
    let status = bin()
        .args(["--definitely-not-a-flag", "in.html", "out.png"])
        .status()
        .expect("spawn");
    assert!(!status.success());
}

#[test]
fn version_is_zero() {
    let status = bin().arg("--version").status().expect("spawn");
    assert!(status.success());
}
