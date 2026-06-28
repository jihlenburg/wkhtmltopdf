// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
fn main() {
    // Probe libqpdf first so we get include/link paths.
    let lib = pkg_config::Config::new()
        .probe("libqpdf")
        .expect("libqpdf not found via pkg-config; install qpdf dev package");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .file("cpp/shim.cpp")
        .include("cpp");

    // Pass every include path that pkg-config reported.
    for path in &lib.include_paths {
        build.include(path);
    }

    build.compile("wkxpdfshim");

    // Emit link search paths and the qpdf library link directive.
    for p in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", p.display());
    }
    println!("cargo:rustc-link-lib=dylib=qpdf");

    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    println!("cargo:rerun-if-changed=cpp/shim.h");
}
