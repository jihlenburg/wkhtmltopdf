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
use wkhtmltox_core::render::Source;
use wkhtmltox_render_chromium::renderer::ChromiumRenderer;

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
            println!(
                "wkhtmltopdf {} (wkhtmltox-rs)",
                env!("CARGO_PKG_VERSION")
            );
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
    let mut renderer = match ChromiumRenderer::spawn() {
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
    eprintln!(
        "wkhtmltopdf: converting {} page(s)…",
        sources.len()
    );
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

    // ── Copy to stdout if requested ───────────────────────────────────────────
    if let Some(ref tf) = stdout_temp {
        match std::fs::read(tf.path()) {
            Ok(bytes) => {
                if let Err(e) = io::stdout().write_all(&bytes) {
                    eprintln!("wkhtmltopdf: stdout write failed: {e}");
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
            Ok(Source::Url(format!("file://{}", abs.display())))
        }
        Input::Stdin => {
            let tf = stdin_temp.expect(
                "stdin_temp must be populated before resolving Stdin inputs",
            );
            Ok(Source::Url(format!("file://{}", tf.path().display())))
        }
    }
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
