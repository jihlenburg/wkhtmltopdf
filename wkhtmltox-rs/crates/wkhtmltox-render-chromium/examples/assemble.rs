// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Assemble multiple HTML inputs into a single PDF via ChromiumRenderer.
//!
//! Usage:
//!   assemble <out.pdf> [FLAGS] <in1.html> [in2.html ...]
//!
//! Flags (must appear before input HTML files):
//!   --toc                       Prepend a generated Table of Contents page.
//!   --cover <path>              Render <path> as the first (cover) page.
//!                               Excluded from headers/footers and TOC.
//!   --footer-center <tmpl>      Template for the center footer cell.
//!                               Supports [page], [topage], [section], [title], etc.
//!   --header-right <tmpl>       Template for the right header cell.
//!   --number                    Stamp a simple centred "N / M" page footer
//!                               (legacy shorthand; overridden by --footer-center).
//!
//! Each HTML input is resolved to an absolute path and loaded as a file:// URL.
//! Page geometry defaults to A4, 10 mm margins, scale 1.0.
//! Prints an AssemblyReport (pages, objects) to stderr on success.

use std::path::PathBuf;
use wkhtmltox_core::assembly::{assemble_pdf, AssembleOpts, CellText};
use wkhtmltox_core::render::{PageGeometry, Source};
use wkhtmltox_render_chromium::renderer::ChromiumRenderer;

fn main() {
    let raw_args: Vec<String> = std::env::args().collect();

    if raw_args.len() < 3 {
        eprintln!(
            "Usage: assemble <out.pdf> [--toc] [--cover <path>] \
             [--footer-center <tmpl>] [--header-right <tmpl>] [--number] \
             <in1.html> [in2.html ...]"
        );
        std::process::exit(1);
    }

    let out = PathBuf::from(&raw_args[1]);

    // Parse flags and positional HTML inputs from the remaining args.
    let mut with_toc = false;
    let mut number = false;
    let mut cover_path: Option<PathBuf> = None;
    let mut footer_center: Option<String> = None;
    let mut header_right: Option<String> = None;
    let mut input_paths: Vec<PathBuf> = Vec::new();

    let mut i = 2usize;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "--toc" => {
                with_toc = true;
                i += 1;
            }
            "--number" => {
                number = true;
                i += 1;
            }
            "--cover" => {
                i += 1;
                if i >= raw_args.len() {
                    eprintln!("error: --cover requires a path argument");
                    std::process::exit(1);
                }
                cover_path = Some(PathBuf::from(&raw_args[i]));
                i += 1;
            }
            "--footer-center" => {
                i += 1;
                if i >= raw_args.len() {
                    eprintln!("error: --footer-center requires a template argument");
                    std::process::exit(1);
                }
                footer_center = Some(raw_args[i].clone());
                i += 1;
            }
            "--header-right" => {
                i += 1;
                if i >= raw_args.len() {
                    eprintln!("error: --header-right requires a template argument");
                    std::process::exit(1);
                }
                header_right = Some(raw_args[i].clone());
                i += 1;
            }
            arg if arg.starts_with("--") => {
                eprintln!("error: unknown flag {arg:?}");
                std::process::exit(1);
            }
            _ => {
                // Everything else is treated as an HTML input file.
                input_paths.push(PathBuf::from(&raw_args[i]));
                i += 1;
            }
        }
    }

    if input_paths.is_empty() {
        eprintln!("error: at least one HTML input file is required");
        std::process::exit(1);
    }

    // Resolve each HTML input to an absolute path and build Source::Url.
    let mut objects: Vec<Source> = Vec::with_capacity(input_paths.len());
    for p in &input_paths {
        let abs = p.canonicalize().unwrap_or_else(|e| {
            eprintln!("cannot resolve input path {:?}: {e}", p);
            std::process::exit(1);
        });
        objects.push(Source::Url(format!("file://{}", abs.display())));
    }

    // Resolve the optional cover path to an absolute Source::Url.
    let cover_src: Option<Source> = cover_path.map(|p| {
        let abs = p.canonicalize().unwrap_or_else(|e| {
            eprintln!("cannot resolve cover path {:?}: {e}", p);
            std::process::exit(1);
        });
        Source::Url(format!("file://{}", abs.display()))
    });

    // Build AssembleOpts from parsed flags.
    let footer = match footer_center {
        Some(ref tmpl) => Some(CellText {
            left: String::new(),
            center: tmpl.clone(),
            right: String::new(),
        }),
        None => None,
    };
    let header = match header_right {
        Some(ref tmpl) => Some(CellText {
            left: String::new(),
            center: String::new(),
            right: tmpl.clone(),
        }),
        None => None,
    };

    let opts = AssembleOpts {
        number,
        with_toc,
        cover: cover_src,
        footer,
        header,
        ..Default::default()
    };

    let mut renderer = ChromiumRenderer::spawn().unwrap_or_else(|e| {
        eprintln!("failed to spawn ChromiumRenderer: {e}");
        std::process::exit(1);
    });

    let geom = PageGeometry {
        prefer_css_page_size: false,
        ..Default::default()
    };

    let report = assemble_pdf(&mut renderer, &objects, &geom, &out, &opts).unwrap_or_else(|e| {
        eprintln!("assemble_pdf failed: {e}");
        std::process::exit(1);
    });

    eprintln!(
        "assembled {} object(s) -> {} page(s) -> {}",
        report.objects,
        report.pages,
        out.display()
    );
}
