// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// wkhtmltopdf-cli/src/main.rs — wkhtmltopdf binary entry point.
//
// Parses argv → ParsedInvocation (via wkhtmltox_cli::parse), then:
//   - info modes (--help, --version, …) → print + exit 0.
//   - Convert mode → resolve inputs to Sources, drive ChromiumRenderer +
//     assemble_pdf, write PDF to file or stdout.

#![forbid(unsafe_code)]

use std::io::{self, Read as _, Write as _};
use std::path::PathBuf;

use tempfile::NamedTempFile;
use wkhtmltox_cli::{help_text, parse, Input, Output, RunMode};
use wkhtmltox_core::assembly::assemble_pdf;
use wkhtmltox_core::pdfread::extract_outline;
use wkhtmltox_core::render::Source;
use wkhtmltox_core::tocxsl::{default_toc_xsl, outline_to_xml, TocXslSettings};
use wkhtmltox_render_chromium::renderer::{ChromiumRenderer, SpawnOpts};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    // ── Parse argv ────────────────────────────────────────────────────────────
    let inv = match parse(args) {
        Ok(inv) => inv,
        Err(e) => {
            eprintln!("wkhtmltopdf: {e}");
            eprintln!("Use --help for usage.");
            return 1;
        }
    };

    // ── Info modes ────────────────────────────────────────────────────────────
    match inv.mode {
        RunMode::Version => {
            println!("wkhtmltopdf {} (wkhtmltox-rs)", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        RunMode::Help => {
            print!("{}", help_text(false));
            return 0;
        }
        RunMode::ExtendedHelp => {
            print!("{}", help_text(true));
            return 0;
        }
        // Readme and manpage: print the extended help text as a best-effort
        // substitute (full man-page generation is deferred to a later task).
        RunMode::Readme | RunMode::Manpage => {
            print!("{}", help_text(true));
            return 0;
        }
        RunMode::DumpDefaultTocXsl => {
            // Print the default TOC XSLT stylesheet to stdout and exit 0.
            // No rendering is performed.
            print!("{}", default_toc_xsl(&TocXslSettings::default()));
            return 0;
        }
        RunMode::Convert => {}
    }

    // ── Emit parse warnings ───────────────────────────────────────────────────
    for w in &inv.warnings {
        eprintln!("wkhtmltopdf: warning: {w}");
    }

    // ── Validate: at least one page object ───────────────────────────────────
    if inv.objects.is_empty() {
        eprintln!("wkhtmltopdf: no input objects specified");
        eprintln!("Use --help for usage.");
        return 1;
    }

    // ── Validate: File inputs must exist on disk (Fix 2) ─────────────────────
    for (_, inp) in &inv.objects {
        if let Input::File(path) = inp {
            if !std::path::Path::new(path).exists() {
                eprintln!("wkhtmltopdf: error: input file not found: {path}");
                return 1;
            }
        }
    }
    if let Some(Input::File(path)) = &inv.cover {
        if !std::path::Path::new(path).exists() {
            eprintln!("wkhtmltopdf: error: cover file not found: {path}");
            return 1;
        }
    }

    // ── Handle stdin inputs ───────────────────────────────────────────────────
    // Read stdin once and write it to a temp .html file so it can be loaded
    // via a file:// URL.  The file guard is kept alive until after assembly.
    let stdin_temp: Option<NamedTempFile> = {
        let needs_stdin = inv.objects.iter().any(|(_, i)| matches!(i, Input::Stdin))
            || matches!(&inv.cover, Some(Input::Stdin));
        if needs_stdin {
            match read_stdin_to_temp() {
                Ok(f) => Some(f),
                Err(e) => {
                    eprintln!("wkhtmltopdf: failed to read stdin: {e}");
                    return 1;
                }
            }
        } else {
            None
        }
    };

    // ── Resolve inputs to Sources ─────────────────────────────────────────────
    let mut sources: Vec<Source> = Vec::with_capacity(inv.objects.len());
    for (_, inp) in &inv.objects {
        match resolve_input(inp, stdin_temp.as_ref()) {
            Ok(src) => sources.push(src),
            Err(e) => {
                eprintln!("wkhtmltopdf: {e}");
                return 1;
            }
        }
    }

    // ── Build geometry and assembly options ───────────────────────────────────
    let geom = inv.global.to_geometry();
    let mut opts = inv.global.to_assemble_opts();

    // CLI `--toc` / `toc` keyword overrides the global setting.
    opts.with_toc = inv.toc;

    // CLI `cover <input>` overrides any cover URL from global settings.
    if let Some(cover_inp) = &inv.cover {
        match resolve_input(cover_inp, stdin_temp.as_ref()) {
            Ok(src) => opts.cover = Some(src),
            Err(e) => {
                eprintln!("wkhtmltopdf: cover: {e}");
                return 1;
            }
        }
    }

    // ── Spawn Chromium renderer ───────────────────────────────────────────────
    eprintln!("wkhtmltopdf: starting renderer…");
    let proxy = inv.global.proxy.clone();
    let mut renderer = match ChromiumRenderer::spawn_opts(SpawnOpts { proxy }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("wkhtmltopdf: failed to start renderer: {e}");
            return 1;
        }
    };

    // ── Determine output path ─────────────────────────────────────────────────
    // For stdout output we write to a temp file and copy the bytes afterwards.
    let stdout_temp: Option<NamedTempFile>;
    let out_path: PathBuf;

    match &inv.output {
        Output::Path(p) => {
            stdout_temp = None;
            out_path = PathBuf::from(p);
        }
        Output::Stdout => {
            let tf = match tempfile::Builder::new()
                .prefix("wkx-out-")
                .suffix(".pdf")
                .tempfile()
            {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("wkhtmltopdf: failed to create output tempfile: {e}");
                    return 1;
                }
            };
            out_path = tf.path().to_path_buf();
            stdout_temp = Some(tf);
        }
    }

    // ── Assemble PDF ──────────────────────────────────────────────────────────
    eprintln!("wkhtmltopdf: converting {} page(s)…", sources.len());
    match assemble_pdf(&mut renderer, &sources, &geom, &out_path, &opts) {
        Ok(report) => {
            eprintln!(
                "wkhtmltopdf: done — {} page(s) from {} object(s)",
                report.pages, report.objects
            );
        }
        Err(e) => {
            eprintln!("wkhtmltopdf: conversion failed: {e}");
            return 1;
        }
    }

    // ── --dump-outline: write outline XML extracted from the assembled PDF ────
    // The outline is read from the final PDF's /Outlines bookmark tree.
    // v1 limitation: the outline reflects the PDF's embedded bookmarks; it may
    // be empty if the document contains no headings recognised by the renderer.
    if let Some(dump_path) = &inv.dump_outline {
        let entries = extract_outline(&out_path);
        let xml = outline_to_xml(&entries);
        if let Err(e) = std::fs::write(dump_path, xml.as_bytes()) {
            eprintln!("wkhtmltopdf: warning: --dump-outline: failed to write {dump_path:?}: {e}");
        }
    }

    // ── Copy to stdout if requested ───────────────────────────────────────────
    if let Some(ref tf) = stdout_temp {
        match std::fs::read(tf.path()) {
            Ok(bytes) => {
                if let Err(e) = io::stdout().write_all(&bytes) {
                    eprintln!("wkhtmltopdf: stdout write failed: {e}");
                    return 1;
                }
                // Flush before process::exit skips destructors (Fix 3).
                if let Err(e) = io::stdout().flush() {
                    eprintln!("wkhtmltopdf: stdout flush failed: {e}");
                    return 1;
                }
            }
            Err(e) => {
                eprintln!("wkhtmltopdf: failed to read output tempfile: {e}");
                return 1;
            }
        }
    }

    // Keep temp guards alive through the end of `run` (dropped here).
    drop(stdin_temp);
    drop(stdout_temp);

    0
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a CLI [`Input`] to a renderer [`Source`].
///
/// `stdin_temp` must be `Some` when `inp` is [`Input::Stdin`]; callers must
/// call [`read_stdin_to_temp`] first.
fn resolve_input(inp: &Input, stdin_temp: Option<&NamedTempFile>) -> Result<Source, String> {
    match inp {
        Input::Url(u) => Ok(Source::Url(u.clone())),
        Input::File(p) => {
            let abs = make_absolute(p)?;
            // Percent-encode path so spaces, #, ?, % and other special bytes
            // are valid in the file:// URL (Fix 4).
            Ok(Source::Url(format!("file://{}", percent_encode_path(&abs))))
        }
        Input::Stdin => {
            // Return Err instead of panicking when the caller forgot to call
            // read_stdin_to_temp (Fix 5).
            let tf = stdin_temp.ok_or_else(|| {
                "internal error: stdin_temp not populated before resolving Stdin inputs".to_string()
            })?;
            Ok(Source::Url(format!(
                "file://{}",
                percent_encode_path(tf.path())
            )))
        }
    }
}

/// Percent-encode a file-system path for use in a `file://` URL (Fix 4).
///
/// Keeps unreserved URI characters (RFC 3986 §2.3) and the path-safe bytes
/// `/`, `:`, and `@` unencoded; encodes everything else as `%XX`.  In
/// practice this covers space, `#`, `?`, `%`, and any non-ASCII byte.
fn percent_encode_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 16);
    for &byte in s.as_bytes() {
        match byte {
            // Unreserved (RFC 3986 §2.3) + path-safe chars kept as-is.
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'/'
            | b':'
            | b'@' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Read all of stdin and write it to a named temp file with a `.html` suffix.
///
/// The returned [`NamedTempFile`] guard keeps the file on disk; callers must
/// hold it alive until after the renderer has finished reading the URL.
fn read_stdin_to_temp() -> Result<NamedTempFile, io::Error> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    let mut tf = tempfile::Builder::new()
        .prefix("wkx-stdin-")
        .suffix(".html")
        .tempfile()?;
    tf.write_all(&bytes)?;
    tf.flush()?;
    Ok(tf)
}

/// Return an absolute path from `p` without requiring the path to exist.
///
/// If `p` is already absolute it is returned as-is.  Otherwise the current
/// working directory is prepended.
fn make_absolute(p: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .map_err(|e| format!("cannot determine current directory: {e}"))
    }
}
