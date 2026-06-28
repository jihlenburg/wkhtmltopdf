// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Minimal CLI: render <input.html> to <output.pdf> via ChromiumRenderer.
//! Geometry matches wkhtmltopdf defaults: A4, 10 mm margins, scale 1.0,
//! print_background true, prefer_css_page_size false, generate_document_outline true.

use std::path::PathBuf;
use wkhtmltox_core::render::*;
use wkhtmltox_render_chromium::renderer::ChromiumRenderer;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: render <input.html> <output.pdf>");
        std::process::exit(1);
    }

    let input = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);

    let abs_input = input
        .canonicalize()
        .unwrap_or_else(|e| { eprintln!("cannot resolve input path: {e}"); std::process::exit(1); });

    let url = format!("file://{}", abs_input.display());

    let mut renderer = ChromiumRenderer::spawn()
        .unwrap_or_else(|e| { eprintln!("failed to spawn ChromiumRenderer: {e}"); std::process::exit(1); });

    let page = renderer
        .open(
            &Source::Url(url),
            &LoadSettings {
                enable_javascript: true,
                allow_local_file_access: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| { eprintln!("open failed: {e}"); std::process::exit(1); });

    renderer
        .wait_ready(page, &ReadyPolicy::default())
        .unwrap_or_else(|e| { eprintln!("wait_ready failed: {e}"); std::process::exit(1); });

    let geom = PageGeometry {
        width_mm: 210.0,
        height_mm: 297.0,
        margin_top_mm: 10.0,
        margin_bottom_mm: 10.0,
        margin_left_mm: 10.0,
        margin_right_mm: 10.0,
        orientation: Orientation::Portrait,
        scale: 1.0,
        print_background: true,
        prefer_css_page_size: false,
        generate_document_outline: true,
    };

    let pdf_bytes = renderer
        .print_pdf(page, &geom)
        .unwrap_or_else(|e| { eprintln!("print_pdf failed: {e}"); std::process::exit(1); });

    std::fs::write(&output, &pdf_bytes)
        .unwrap_or_else(|e| { eprintln!("write output failed: {e}"); std::process::exit(1); });

    eprintln!("wrote {} bytes -> {}", pdf_bytes.len(), output.display());
}
