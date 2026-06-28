// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Integration tests for the wkhtmltopdf binary.
//
// Chrome-gated tests are marked `#[ignore]`; run them with:
//   cargo test -p wkhtmltopdf-cli -- --ignored

use std::process::Command;
use std::time::Duration;

/// Path to the wkhtmltopdf binary built by Cargo for this package.
const BIN: &str = env!("CARGO_BIN_EXE_wkhtmltopdf");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run_bin(args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("failed to run wkhtmltopdf binary")
}

/// Write a minimal HTML file to a NamedTempFile and return the guard + path.
fn write_temp_html(content: &str) -> tempfile::NamedTempFile {
    use std::io::Write as _;
    let mut tf = tempfile::Builder::new()
        .prefix("wkx-test-")
        .suffix(".html")
        .tempfile()
        .expect("create temp html");
    tf.write_all(content.as_bytes()).expect("write html");
    tf.flush().expect("flush");
    tf
}

// ---------------------------------------------------------------------------
// Chrome-gated integration tests
// ---------------------------------------------------------------------------

/// Convert a small HTML file to PDF and verify the output starts with %PDF.
#[test]
#[ignore]
fn convert_html_to_pdf_starts_with_pdf_magic() {
    let html = write_temp_html(
        "<!DOCTYPE html><html><body><h1>Hello PDF</h1></body></html>",
    );
    let out_pdf = tempfile::Builder::new()
        .prefix("wkx-out-")
        .suffix(".pdf")
        .tempfile()
        .expect("create temp pdf");
    let out_path = out_pdf.path().to_str().expect("path to str").to_owned();

    // Persist the html tempfile guard until after the command.
    let in_path = html.path().to_str().expect("html path").to_owned();

    let out = run_bin(&[&in_path, &out_path]);

    assert!(
        out.status.success(),
        "wkhtmltopdf exited with non-zero status: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );

    let pdf_bytes = std::fs::read(&out_path).expect("read output pdf");
    assert!(
        pdf_bytes.starts_with(b"%PDF"),
        "output file does not start with %PDF magic bytes; first 8 bytes: {:?}",
        &pdf_bytes[..pdf_bytes.len().min(8)],
    );

    drop(html);
    drop(out_pdf);
}

/// --version prints a version string and exits with status 0.
#[test]
#[ignore]
fn version_flag_prints_and_exits_zero() {
    let out = run_bin(&["--version"]);
    assert!(
        out.status.success(),
        "--version exited non-zero: {}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("wkhtmltopdf"),
        "--version output should contain 'wkhtmltopdf', got: {stdout:?}"
    );
    // Should contain a version number (digit-dot-digit pattern).
    assert!(
        stdout.chars().any(|c| c.is_ascii_digit()),
        "--version output should contain a version number, got: {stdout:?}"
    );
}

/// An unknown flag causes a non-zero exit.
#[test]
#[ignore]
fn unknown_flag_exits_nonzero() {
    let out = run_bin(&["--frobnicate"]);
    assert!(
        !out.status.success(),
        "unknown flag should cause non-zero exit, but got status {}",
        out.status
    );
}

/// --help exits 0 and prints usage.
#[test]
#[ignore]
fn help_flag_exits_zero_and_prints_usage() {
    let out = run_bin(&["--help"]);
    assert!(
        out.status.success(),
        "--help exited non-zero: {}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Usage"),
        "--help should print 'Usage', got: {stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// Timeout guard: make sure tests don't hang CI forever.
// ---------------------------------------------------------------------------

/// Verify that the binary exits quickly for info flags (no Chrome needed).
/// This is NOT ignore-gated because it doesn't need Chrome.
#[test]
fn version_flag_exits_promptly() {
    use std::time::Instant;
    let start = Instant::now();
    let out = run_bin(&["--version"]);
    let elapsed = start.elapsed();
    assert!(
        out.status.success(),
        "--version exited non-zero: {}",
        out.status
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "--version took too long: {elapsed:?}"
    );
}

/// Unknown flag → non-zero exit, also without Chrome (pure arg parse).
#[test]
fn unknown_flag_exits_nonzero_fast() {
    let out = run_bin(&["--frobnicate"]);
    assert!(
        !out.status.success(),
        "unknown flag should cause non-zero exit; got {}",
        out.status
    );
}

/// Bad flag value (--page-size with unknown size) → non-zero exit (Fix 1).
#[test]
fn bad_flag_value_exits_nonzero() {
    let out = run_bin(&["--page-size", "Quux", "page.html", "out.pdf"]);
    assert!(
        !out.status.success(),
        "bad flag value should cause non-zero exit; got {}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid value for --page-size"),
        "stderr should mention the flag name, got: {stderr:?}"
    );
}

/// Missing input file → non-zero exit (Fix 2).
#[test]
fn missing_input_file_exits_nonzero() {
    let out = run_bin(&["/nonexistent/path/absolutely-missing.html", "out.pdf"]);
    assert!(
        !out.status.success(),
        "missing input should cause non-zero exit; got {}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not found"),
        "stderr should mention 'not found', got: {stderr:?}"
    );
}
