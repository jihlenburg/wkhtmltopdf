// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Assemble multiple HTML inputs into a single PDF via ChromiumRenderer.
//!
//! Usage: assemble <out.pdf> <in1.html> [in2.html ...]
//!
//! Each input is resolved to an absolute path and loaded as a file:// URL.
//! Page geometry defaults to A4, 10 mm margins, scale 1.0.
//! Page-number footers are stamped (number=true).
//! Prints an AssemblyReport (pages, objects) to stderr on success.

use std::path::PathBuf;
use wkhtmltox_core::assembly::{assemble_pdf, AssembleOpts};
use wkhtmltox_core::render::{PageGeometry, Source};
use wkhtmltox_render_chromium::renderer::ChromiumRenderer;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: assemble <out.pdf> <in1.html> [in2.html ...]");
        std::process::exit(1);
    }

    let out = PathBuf::from(&args[1]);
    let inputs = &args[2..];

    // Resolve each input to an absolute path and build Source::Url.
    let mut objects: Vec<Source> = Vec::with_capacity(inputs.len());
    for raw in inputs {
        let p = PathBuf::from(raw);
        let abs = p.canonicalize().unwrap_or_else(|e| {
            eprintln!("cannot resolve input path {:?}: {e}", raw);
            std::process::exit(1);
        });
        objects.push(Source::Url(format!("file://{}", abs.display())));
    }

    let mut renderer = ChromiumRenderer::spawn().unwrap_or_else(|e| {
        eprintln!("failed to spawn ChromiumRenderer: {e}");
        std::process::exit(1);
    });

    let geom = PageGeometry {
        prefer_css_page_size: false,
        ..Default::default()
    };

    let report = assemble_pdf(
        &mut renderer,
        &objects,
        &geom,
        &out,
        &AssembleOpts { number: true, ..Default::default() },
    )
    .unwrap_or_else(|e| {
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
