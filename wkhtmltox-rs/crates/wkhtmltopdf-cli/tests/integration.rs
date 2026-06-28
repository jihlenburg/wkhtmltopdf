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
    let html = write_temp_html("<!DOCTYPE html><html><body><h1>Hello PDF</h1></body></html>");
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

// ---------------------------------------------------------------------------
// TOC XSL dump utilities (no Chrome required)
// ---------------------------------------------------------------------------

/// `--dump-default-toc-xsl` exits 0 and prints a valid XSL stylesheet.
/// The output must contain `xsl:stylesheet` and the wkhtmltopdf outline namespace.
#[test]
fn dump_default_toc_xsl_exits_zero_and_prints_xsl() {
    use std::time::Instant;
    let start = Instant::now();
    let out = run_bin(&["--dump-default-toc-xsl"]);
    let elapsed = start.elapsed();

    assert!(
        out.status.success(),
        "--dump-default-toc-xsl exited non-zero: {}",
        out.status
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "--dump-default-toc-xsl took too long: {elapsed:?}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("xsl:stylesheet"),
        "stdout should contain 'xsl:stylesheet', got: {stdout:?}"
    );
    assert!(
        stdout.contains("http://wkhtmltopdf.org/outline"),
        "stdout should contain the wkhtmltopdf outline namespace, got: {stdout:?}"
    );
}

/// `--xsl-style-sheet foo.xsl in.html out.pdf` must not exit with
/// "unknown option" — the flag is now accepted by the parser.
/// (The render itself will fail because foo.xsl does not exist, but the
/// parse step must succeed past the flag.)
#[test]
fn xsl_style_sheet_flag_is_not_unknown_option() {
    // Run with a non-existent file so the binary exits non-zero (render error),
    // but the stderr must NOT contain "unknown option" for --xsl-style-sheet.
    let out = run_bin(&["--xsl-style-sheet", "foo.xsl", "in.html", "out.pdf"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unknown option"),
        "stderr must not contain 'unknown option' for --xsl-style-sheet, got: {stderr:?}"
    );
}
