// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// wkhtmltoimage-cli/src/main.rs — wkhtmltoimage binary entry point.
//
// Grammar: wkhtmltoimage [OPTIONS]... <input url/file/-> <output file/->
//
// Parses argv flags into `ImageGlobalSettings` (via `set_image_global`), then:
//   - Info modes (--help / --version) → print + exit 0.
//   - Convert mode → resolve input, spawn `ChromiumRenderer`, open, wait_ready,
//     snapshot, image::produce, write output bytes.

#![forbid(unsafe_code)]

use std::io::{self, Read as _, Write as _};
use std::path::PathBuf;

use tempfile::NamedTempFile;
use wkhtmltox_core::image::produce;
use wkhtmltox_core::registry::set_image_global;
use wkhtmltox_core::render::{ReadyPolicy, Renderer, SnapshotOpts, Source};
use wkhtmltox_core::settings::ImageGlobalSettings;
use wkhtmltox_render_chromium::renderer::{ChromiumRenderer, SpawnOpts};

// ---------------------------------------------------------------------------
// Run mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Convert,
    Help,
    Version,
}

// ---------------------------------------------------------------------------
// Input / Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Input {
    Url(String),
    File(String),
    Stdin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Output {
    Path(String),
    Stdout,
}

// ---------------------------------------------------------------------------
// Flag table
// ---------------------------------------------------------------------------

enum Action {
    /// Set the named registry setting to the next token (arity 1).
    Setting(&'static str),
    /// Set the named registry setting to a compile-time constant (arity 0).
    Const(&'static str, &'static str),
    /// Special non-registry action (arity 0).
    Special(SpecialKind),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecialKind {
    Help,
    Version,
    Quiet,
    TransparentOn,
}

struct FlagSpec {
    long: &'static str,
    short: Option<char>,
    action: Action,
}

static FLAGS: &[FlagSpec] = &[
    // ── Info / meta ──────────────────────────────────────────────────────────
    FlagSpec {
        long: "help",
        short: Some('h'),
        action: Action::Special(SpecialKind::Help),
    },
    FlagSpec {
        long: "version",
        short: Some('V'),
        action: Action::Special(SpecialKind::Version),
    },
    FlagSpec {
        long: "quiet",
        short: Some('q'),
        action: Action::Special(SpecialKind::Quiet),
    },
    // ── Format / quality ─────────────────────────────────────────────────────
    FlagSpec {
        long: "format",
        short: Some('f'),
        action: Action::Setting("fmt"),
    },
    FlagSpec {
        long: "quality",
        short: None,
        action: Action::Setting("quality"),
    },
    // ── Viewport dimensions ───────────────────────────────────────────────────
    FlagSpec {
        long: "width",
        short: Some('w'),
        action: Action::Setting("screenWidth"),
    },
    FlagSpec {
        long: "height",
        short: None,
        action: Action::Setting("screenHeight"),
    },
    // ── Crop ─────────────────────────────────────────────────────────────────
    FlagSpec {
        long: "crop-x",
        short: None,
        action: Action::Setting("crop.left"),
    },
    FlagSpec {
        long: "crop-y",
        short: None,
        action: Action::Setting("crop.top"),
    },
    FlagSpec {
        long: "crop-w",
        short: None,
        action: Action::Setting("crop.width"),
    },
    FlagSpec {
        long: "crop-h",
        short: None,
        action: Action::Setting("crop.height"),
    },
    // ── Appearance ───────────────────────────────────────────────────────────
    FlagSpec {
        long: "transparent",
        short: None,
        action: Action::Special(SpecialKind::TransparentOn),
    },
    FlagSpec {
        long: "zoom",
        short: None,
        action: Action::Setting("zoom"),
    },
    // ── Load / security ──────────────────────────────────────────────────────
    FlagSpec {
        long: "enable-local-file-access",
        short: None,
        action: Action::Const("load.blockLocalFileAccess", "false"),
    },
    FlagSpec {
        long: "disable-local-file-access",
        short: None,
        action: Action::Const("load.blockLocalFileAccess", "true"),
    },
    FlagSpec {
        long: "allow",
        short: None,
        action: Action::Setting("load.allowedPath"),
    },
    FlagSpec {
        long: "safe",
        short: None,
        action: Action::Const("load.safe", "true"),
    },
    FlagSpec {
        long: "javascript-delay",
        short: None,
        action: Action::Setting("load.jsdelay"),
    },
    // ── Web rendering ────────────────────────────────────────────────────────
    FlagSpec {
        long: "no-images",
        short: None,
        action: Action::Const("web.loadImages", "false"),
    },
];

fn arity_of(action: &Action) -> usize {
    match action {
        Action::Setting(_) => 1,
        Action::Const(_, _) => 0,
        Action::Special(_) => 0,
    }
}

fn find_flag(tok: &str) -> Option<&'static FlagSpec> {
    for spec in FLAGS {
        if tok == format!("--{}", spec.long) {
            return Some(spec);
        }
        if let Some(ch) = spec.short {
            if tok == format!("-{ch}") {
                return Some(spec);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Parsed invocation
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Parsed {
    mode: Mode,
    settings: ImageGlobalSettings,
    input: Option<Input>,
    output: Option<Output>,
    quiet: bool,
}

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

/// Parse a wkhtmltoimage-style argument list.
///
/// Returns `Err(message)` on unknown flags or bad values.
fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut settings = ImageGlobalSettings::default();
    let mut mode = Mode::Convert;
    let mut quiet = false;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let tok = args[i].as_str();

        // Detect flag tokens: --long or -X (single alpha short).
        let is_flag = (tok.starts_with("--") && tok.len() > 2)
            || (tok.starts_with('-') && tok.len() == 2 && tok.as_bytes()[1].is_ascii_alphabetic());

        if !is_flag {
            positionals.push(tok.to_owned());
            i += 1;
            continue;
        }

        let spec = find_flag(tok).ok_or_else(|| format!("unknown option: {tok}"))?;

        let ar = arity_of(&spec.action);
        if i + ar >= args.len() && ar > 0 {
            return Err(format!(
                "option {tok} requires {ar} argument(s) but reached end of input"
            ));
        }
        let val1 = if ar >= 1 { args[i + 1].as_str() } else { "" };

        match &spec.action {
            Action::Special(kind) => match kind {
                SpecialKind::Help => {
                    mode = Mode::Help;
                    return Ok(Parsed {
                        mode,
                        settings,
                        input: None,
                        output: None,
                        quiet,
                    });
                }
                SpecialKind::Version => {
                    mode = Mode::Version;
                    return Ok(Parsed {
                        mode,
                        settings,
                        input: None,
                        output: None,
                        quiet,
                    });
                }
                SpecialKind::Quiet => {
                    quiet = true;
                }
                SpecialKind::TransparentOn => {
                    set_image_global(&mut settings, "transparent", "true")
                        .map_err(|e| format!("--transparent: {e}"))?;
                }
            },
            Action::Setting(name) => {
                set_image_global(&mut settings, name, val1)
                    .map_err(|e| format!("invalid value for --{}: {e}", spec.long))?;
            }
            Action::Const(name, value) => {
                set_image_global(&mut settings, name, value)
                    .map_err(|e| format!("invalid value for --{}: {e}", spec.long))?;
            }
        }

        i += 1 + ar;
    }

    // Resolve positionals: must have exactly 2 (input, output).
    let (input, output) = match positionals.len() {
        2 => {
            let inp = parse_input(&positionals[0]);
            let out = parse_output(&positionals[1]);
            (Some(inp), Some(out))
        }
        n if n < 2 => {
            if mode == Mode::Convert {
                return Err(format!(
                    "expected 2 positional arguments (input output), got {n}"
                ));
            }
            (None, None)
        }
        _ => {
            return Err(format!(
                "expected 2 positional arguments (input output), got {}",
                positionals.len()
            ));
        }
    };

    Ok(Parsed {
        mode,
        settings,
        input,
        output,
        quiet,
    })
}

fn parse_input(s: &str) -> Input {
    if s == "-" {
        Input::Stdin
    } else if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://") {
        Input::Url(s.to_owned())
    } else {
        Input::File(s.to_owned())
    }
}

fn parse_output(s: &str) -> Output {
    if s == "-" {
        Output::Stdout
    } else {
        Output::Path(s.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

/// Generate a human-readable usage string for wkhtmltoimage.
pub fn help_text() -> String {
    let mut out = String::new();
    out.push_str(concat!(
        "Usage:\n",
        "  wkhtmltoimage [OPTIONS]... <input url/file/-> <output file/->\n\n",
        "Convert an HTML page to an image (PNG or JPEG).\n\n",
        "Options:\n",
    ));

    for spec in FLAGS {
        let short_col = if let Some(ch) = spec.short {
            format!("-{ch}, ")
        } else {
            "    ".to_owned()
        };

        let hint: &str = match spec.action {
            Action::Setting(_) => " <arg>",
            _ => "",
        };

        let flag_col = format!("  {}--{}{}", short_col, spec.long, hint);
        out.push_str(&format!("{flag_col}\n"));
    }

    out.push('\n');
    out.push_str("Positional arguments:\n");
    out.push_str("  <input>   URL, file path, or '-' (stdin)\n");
    out.push_str("  <output>  File path or '-' (stdout)\n");
    out
}

// ---------------------------------------------------------------------------
// Main logic
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    // ── Parse argv ────────────────────────────────────────────────────────────
    let mut parsed = match parse(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wkhtmltoimage: {e}");
            eprintln!("Use --help for usage.");
            return 1;
        }
    };

    // ── Info modes ────────────────────────────────────────────────────────────
    match parsed.mode {
        Mode::Help => {
            print!("{}", help_text());
            return 0;
        }
        Mode::Version => {
            println!("wkhtmltoimage {} (wkhtmltox-rs)", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        Mode::Convert => {}
    }

    // ── Unwrap positionals (guaranteed by parse for Convert mode) ─────────────
    let input = parsed
        .input
        .take()
        .expect("input must be Some in Convert mode");
    let output = parsed
        .output
        .take()
        .expect("output must be Some in Convert mode");
    let quiet = parsed.quiet;

    // ── Validate file inputs ──────────────────────────────────────────────────
    if let Input::File(ref path) = input {
        if !std::path::Path::new(path).exists() {
            eprintln!("wkhtmltoimage: error: input file not found: {path}");
            return 1;
        }
    }

    // ── Handle stdin → temp file ──────────────────────────────────────────────
    let stdin_temp: Option<NamedTempFile> = if matches!(input, Input::Stdin) {
        match read_stdin_to_temp() {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("wkhtmltoimage: failed to read stdin: {e}");
                return 1;
            }
        }
    } else {
        None
    };

    // ── Resolve input to Source ───────────────────────────────────────────────
    let source = match resolve_input(&input, stdin_temp.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("wkhtmltoimage: {e}");
            return 1;
        }
    };

    // ── Spawn Chromium renderer ───────────────────────────────────────────────
    if !quiet {
        eprintln!("wkhtmltoimage: starting renderer…");
    }
    let proxy = parsed.settings.proxy.clone();
    let mut renderer = match ChromiumRenderer::spawn_opts(SpawnOpts { proxy }) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("wkhtmltoimage: failed to start renderer: {e}");
            return 1;
        }
    };

    // ── Open page ─────────────────────────────────────────────────────────────
    if !quiet {
        eprintln!("wkhtmltoimage: loading page…");
    }
    let load_settings = parsed.settings.to_load_settings();
    let page = match renderer.open(&source, &load_settings) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wkhtmltoimage: failed to open page: {e}");
            return 1;
        }
    };

    // ── Wait for page ready ───────────────────────────────────────────────────
    let ready_policy = ReadyPolicy {
        javascript_delay_ms: parsed.settings.javascript_delay_ms,
        window_status: None,
    };
    if let Err(e) = renderer.wait_ready(page, &ready_policy) {
        eprintln!("wkhtmltoimage: wait_ready failed: {e}");
        return 1;
    }

    // ── Snapshot ──────────────────────────────────────────────────────────────
    if !quiet {
        eprintln!("wkhtmltoimage: capturing screenshot…");
    }
    let image_opts = parsed.settings.to_image_opts();
    let snap_opts = SnapshotOpts {
        format: image_opts.format,
        crop: image_opts.crop,
        scale: image_opts.zoom,
        quality: image_opts.quality,
    };
    let raw_image = match renderer.snapshot(page, &snap_opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("wkhtmltoimage: snapshot failed: {e}");
            return 1;
        }
    };

    // ── Post-process image ────────────────────────────────────────────────────
    // CDP already applied the crop via the clip parameter in SnapshotOpts;
    // produce must not re-crop the already-cropped image.
    let mut produce_opts = image_opts;
    produce_opts.crop = None;
    let image_bytes = match produce(&raw_image, &produce_opts) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("wkhtmltoimage: image pipeline failed: {e}");
            return 1;
        }
    };

    // ── Write output ──────────────────────────────────────────────────────────
    match output {
        Output::Path(ref path) => {
            if let Err(e) = std::fs::write(path, &image_bytes) {
                eprintln!("wkhtmltoimage: failed to write output: {e}");
                return 1;
            }
            if !quiet {
                eprintln!("wkhtmltoimage: done — wrote {path}");
            }
        }
        Output::Stdout => {
            if let Err(e) = io::stdout().write_all(&image_bytes) {
                eprintln!("wkhtmltoimage: stdout write failed: {e}");
                return 1;
            }
            if let Err(e) = io::stdout().flush() {
                eprintln!("wkhtmltoimage: stdout flush failed: {e}");
                return 1;
            }
        }
    }

    // Keep temp guard alive until after rendering is complete.
    drop(stdin_temp);

    0
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a CLI `Input` to a renderer `Source`.
fn resolve_input(inp: &Input, stdin_temp: Option<&NamedTempFile>) -> Result<Source, String> {
    match inp {
        Input::Url(u) => Ok(Source::Url(u.clone())),
        Input::File(p) => {
            let abs = make_absolute(p)?;
            Ok(Source::Url(format!("file://{}", percent_encode_path(&abs))))
        }
        Input::Stdin => {
            let tf = stdin_temp.ok_or_else(|| {
                "internal error: stdin_temp not populated for Stdin input".to_string()
            })?;
            Ok(Source::Url(format!(
                "file://{}",
                percent_encode_path(tf.path())
            )))
        }
    }
}

/// Percent-encode a file-system path for use in a `file://` URL.
///
/// Keeps unreserved URI characters (RFC 3986 §2.3) plus `/`, `:`, `@`;
/// encodes everything else as `%XX`.
fn percent_encode_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 16);
    for &byte in s.as_bytes() {
        match byte {
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

/// Read stdin to a named temp file with a `.html` suffix.
fn read_stdin_to_temp() -> Result<NamedTempFile, io::Error> {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    let mut tf = tempfile::Builder::new()
        .prefix("wkimg-stdin-")
        .suffix(".html")
        .tempfile()?;
    tf.write_all(&bytes)?;
    tf.flush()?;
    Ok(tf)
}

/// Make an absolute path from `p` without requiring the path to exist.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wkhtmltox_core::render::ImageFormat;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ── Unit: help text ───────────────────────────────────────────────────────

    #[test]
    fn help_text_is_non_empty() {
        let text = help_text();
        assert!(!text.is_empty(), "help text must not be empty");
    }

    #[test]
    fn help_text_mentions_key_flags() {
        let text = help_text();
        assert!(text.contains("--format"), "help text must mention --format");
        assert!(
            text.contains("--quality"),
            "help text must mention --quality"
        );
        assert!(text.contains("--width"), "help text must mention --width");
        assert!(
            text.contains("--transparent"),
            "help text must mention --transparent"
        );
        assert!(text.contains("--help"), "help text must mention --help");
        assert!(
            text.contains("--version"),
            "help text must mention --version"
        );
    }

    #[test]
    fn help_text_mentions_usage() {
        let text = help_text();
        assert!(text.contains("Usage"), "help text must contain 'Usage'");
        assert!(
            text.contains("wkhtmltoimage"),
            "help text must mention the binary name"
        );
    }

    // ── Unit: flag → setting mapping ─────────────────────────────────────────

    #[test]
    fn format_png_flag() {
        let parsed = parse(&args(&["--format", "png", "in.html", "out.png"])).unwrap();
        assert_eq!(parsed.settings.fmt, "png");
    }

    #[test]
    fn format_short_flag() {
        let parsed = parse(&args(&["-f", "jpeg", "in.html", "out.jpg"])).unwrap();
        assert_eq!(parsed.settings.fmt, "jpeg");
    }

    #[test]
    fn quality_flag() {
        let parsed = parse(&args(&["--quality", "80", "in.html", "out.jpg"])).unwrap();
        assert_eq!(parsed.settings.quality, 80);
    }

    #[test]
    fn width_flag_maps_to_screen_width() {
        let parsed = parse(&args(&["--width", "800", "in.html", "out.png"])).unwrap();
        assert_eq!(parsed.settings.screen_width, Some(800));
    }

    #[test]
    fn width_short_flag() {
        let parsed = parse(&args(&["-w", "1280", "in.html", "out.png"])).unwrap();
        assert_eq!(parsed.settings.screen_width, Some(1280));
    }

    #[test]
    fn height_flag() {
        let parsed = parse(&args(&["--height", "600", "in.html", "out.png"])).unwrap();
        assert_eq!(parsed.settings.screen_height, Some(600));
    }

    #[test]
    fn crop_flags() {
        let parsed = parse(&args(&[
            "--crop-x", "10", "--crop-y", "20", "--crop-w", "300", "--crop-h", "200", "in.html",
            "out.png",
        ]))
        .unwrap();
        assert_eq!(parsed.settings.crop_x, Some(10));
        assert_eq!(parsed.settings.crop_y, Some(20));
        assert_eq!(parsed.settings.crop_w, Some(300));
        assert_eq!(parsed.settings.crop_h, Some(200));
    }

    #[test]
    fn transparent_flag() {
        let parsed = parse(&args(&["--transparent", "in.html", "out.png"])).unwrap();
        assert!(parsed.settings.transparent);
    }

    #[test]
    fn zoom_flag() {
        let parsed = parse(&args(&["--zoom", "1.5", "in.html", "out.png"])).unwrap();
        assert!((parsed.settings.zoom - 1.5).abs() < 1e-9);
    }

    #[test]
    fn safe_flag() {
        let parsed = parse(&args(&["--safe", "in.html", "out.png"])).unwrap();
        assert!(parsed.settings.safe_mode);
    }

    #[test]
    fn javascript_delay_flag() {
        let parsed = parse(&args(&["--javascript-delay", "500", "in.html", "out.png"])).unwrap();
        assert_eq!(parsed.settings.javascript_delay_ms, 500);
    }

    #[test]
    fn enable_local_file_access_flag() {
        let parsed = parse(&args(&["--enable-local-file-access", "in.html", "out.png"])).unwrap();
        assert!(parsed.settings.allow_local_file_access);
    }

    #[test]
    fn disable_local_file_access_flag() {
        let parsed = parse(&args(&[
            "--disable-local-file-access",
            "in.html",
            "out.png",
        ]))
        .unwrap();
        assert!(!parsed.settings.allow_local_file_access);
    }

    #[test]
    fn quiet_flag() {
        let parsed = parse(&args(&["--quiet", "in.html", "out.png"])).unwrap();
        assert!(parsed.quiet);
    }

    #[test]
    fn quiet_short_flag() {
        let parsed = parse(&args(&["-q", "in.html", "out.png"])).unwrap();
        assert!(parsed.quiet);
    }

    // ── Unit: input / output parsing ─────────────────────────────────────────

    #[test]
    fn url_input_detected() {
        let parsed = parse(&args(&["https://example.com", "out.png"])).unwrap();
        assert_eq!(parsed.input, Some(Input::Url("https://example.com".into())));
    }

    #[test]
    fn file_input_detected() {
        let parsed = parse(&args(&["page.html", "out.png"])).unwrap();
        assert_eq!(parsed.input, Some(Input::File("page.html".into())));
    }

    #[test]
    fn stdin_input_detected() {
        let parsed = parse(&args(&["-", "out.png"])).unwrap();
        assert_eq!(parsed.input, Some(Input::Stdin));
    }

    #[test]
    fn stdout_output_detected() {
        let parsed = parse(&args(&["in.html", "-"])).unwrap();
        assert_eq!(parsed.output, Some(Output::Stdout));
    }

    // ── Unit: error cases ─────────────────────────────────────────────────────

    #[test]
    fn unknown_flag_is_error() {
        let err = parse(&args(&["--frobnicate", "in.html", "out.png"])).unwrap_err();
        assert!(err.contains("unknown option"), "got: {err}");
    }

    #[test]
    fn bad_quality_value_is_error() {
        let err = parse(&args(&["--quality", "not-a-number", "in.html", "out.png"])).unwrap_err();
        assert!(err.contains("--quality"), "got: {err}");
    }

    #[test]
    fn bad_width_value_is_error() {
        let err = parse(&args(&["--width", "abc", "in.html", "out.png"])).unwrap_err();
        assert!(err.contains("--width"), "got: {err}");
    }

    #[test]
    fn missing_positionals_is_error() {
        let err = parse(&args(&[])).unwrap_err();
        assert!(err.contains("positional"), "got: {err}");
    }

    #[test]
    fn only_one_positional_is_error() {
        let err = parse(&args(&["in.html"])).unwrap_err();
        assert!(err.contains("positional"), "got: {err}");
    }

    #[test]
    fn too_many_positionals_is_error() {
        let err = parse(&args(&["in.html", "out.png", "extra"])).unwrap_err();
        assert!(err.contains("positional"), "got: {err}");
    }

    // ── Unit: info modes ──────────────────────────────────────────────────────

    #[test]
    fn help_mode() {
        let parsed = parse(&args(&["--help"])).unwrap();
        assert_eq!(parsed.mode, Mode::Help);
    }

    #[test]
    fn help_short_mode() {
        let parsed = parse(&args(&["-h"])).unwrap();
        assert_eq!(parsed.mode, Mode::Help);
    }

    #[test]
    fn version_mode() {
        let parsed = parse(&args(&["--version"])).unwrap();
        assert_eq!(parsed.mode, Mode::Version);
    }

    #[test]
    fn version_short_mode() {
        let parsed = parse(&args(&["-V"])).unwrap();
        assert_eq!(parsed.mode, Mode::Version);
    }

    // ── Unit: to_image_opts round-trip ────────────────────────────────────────

    #[test]
    fn to_image_opts_jpeg() {
        let parsed = parse(&args(&[
            "-f",
            "jpeg",
            "--quality",
            "75",
            "in.html",
            "out.jpg",
        ]))
        .unwrap();
        let opts = parsed.settings.to_image_opts();
        assert!(matches!(opts.format, ImageFormat::Jpeg));
        assert_eq!(opts.quality, 75);
    }

    #[test]
    fn to_image_opts_png_default() {
        let parsed = parse(&args(&["in.html", "out.png"])).unwrap();
        let opts = parsed.settings.to_image_opts();
        assert!(matches!(opts.format, ImageFormat::Png));
    }
}
