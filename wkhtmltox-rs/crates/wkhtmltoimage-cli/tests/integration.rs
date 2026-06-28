// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Gated integration tests for the wkhtmltoimage binary.
//
// These tests invoke the binary via process::Command and require a live
// Chrome/Chromium installation.  They are marked `#[ignore]` so they are
// skipped by `cargo test` by default.
//
// Run them with:
//   cargo test -p wkhtmltoimage-cli -- --ignored

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_wkhtmltoimage");

/// Render a trivial HTML page to PNG and verify the magic bytes.
#[test]
#[ignore]
fn gated_render_html_to_png() {
    let dir = tempfile::tempdir().expect("tempdir");
    let in_path = dir.path().join("test.html");
    let out_path = dir.path().join("out.png");
    std::fs::write(
        &in_path,
        b"<html><body><p>hello wkhtmltoimage</p></body></html>",
    )
    .expect("write html");

    let status = Command::new(BIN)
        .args([
            "--quiet",
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn wkhtmltoimage");

    assert!(
        status.success(),
        "wkhtmltoimage should exit 0, got: {status:?}"
    );

    let bytes = std::fs::read(&out_path).expect("read output png");
    assert!(bytes.len() > 8, "output must be larger than 8 bytes");
    assert_eq!(
        &bytes[..4],
        b"\x89PNG",
        "output must start with PNG magic bytes"
    );
}

/// --version exits 0.
#[test]
#[ignore]
fn gated_version_exits_zero() {
    let status = Command::new(BIN)
        .arg("--version")
        .status()
        .expect("spawn wkhtmltoimage --version");

    assert!(status.success(), "--version should exit 0, got: {status:?}");
}

/// Unknown flag exits nonzero.
#[test]
#[ignore]
fn gated_unknown_flag_exits_nonzero() {
    let status = Command::new(BIN)
        .args(["--frobnicate", "in.html", "out.png"])
        .status()
        .expect("spawn wkhtmltoimage --frobnicate");

    assert!(
        !status.success(),
        "--frobnicate should exit nonzero, got: {status:?}"
    );
}

/// Missing input file exits nonzero.
#[test]
#[ignore]
fn gated_missing_input_exits_nonzero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out_path = dir.path().join("out.png");

    let status = Command::new(BIN)
        .args([
            "--quiet",
            "/nonexistent/path/that/does/not/exist.html",
            out_path.to_str().unwrap(),
        ])
        .status()
        .expect("spawn wkhtmltoimage with missing input");

    assert!(
        !status.success(),
        "missing input should exit nonzero, got: {status:?}"
    );
}
