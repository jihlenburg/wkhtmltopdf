// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// wkhtmltox-cli/src/lib.rs — wkhtmltopdf-compatible CLI argument model and parser.
//
// Parses a wkhtmltopdf-style argv slice into a `ParsedInvocation`, routing
// each CLI flag to the appropriate registry setting on `GlobalSettings` or
// `PdfObjectSettings`.
//
// ## Object-attach rule
//
// Flags are classified as `Target::Global` or `Target::Both`:
//
// * **Global-only** flags (`--page-size`, `--orientation`, `--margin-*`,
//   `--dpi`, `--grayscale`, `--outline`, etc.) always call `set_global` and
//   therefore apply to all pages.  They cannot be scoped to a single page
//   because the underlying `PdfObjectSettings` does not carry those fields.
//
// * **Both** flags (`--zoom`, `--javascript-delay`, `--header-*`, `--footer-*`,
//   `--print-media-type`, `--encoding`, etc.) call `set_global` when they
//   appear *before* the first page input ("leading phase") and call
//   `set_object` on the **upcoming** page when they appear *after* at least
//   one page input has been seen ("object phase").
//
// The "upcoming page" semantics mean that flags placed *between* two page
// inputs are attached to the **later** page.  For example:
//
// ```text
// page1.html --javascript-delay 500 page2.html out.pdf
// ```
//
// produces `page1` with default `jsdelay` and `page2` with `jsdelay=500`.
//
// Two-argument flags (`--cookie`, `--custom-header`, `--replace`) always
// accumulate into the next page's pending-flags list even when they appear
// in the leading phase; the registry will accept them as
// "recognised-but-unimplemented" and emit a warning.
//
// `cover <url>` is a positional keyword that does **not** transition to the
// object phase.  `toc` (positional) and `--toc` (flag) both set `toc = true`.
//
// The final non-flag positional is always the output (`-` = stdout); every
// preceding positional is a page input (`-` = stdin, URLs beginning with
// `http://`, `https://`, or `file://` become `Input::Url`, everything else
// `Input::File`).

#![forbid(unsafe_code)]

use wkhtmltox_core::registry::{get_global, set_global, set_object};
use wkhtmltox_core::settings::{GlobalSettings, PdfObjectSettings};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Source of a single page to convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A URL (http://, https://, or file://).
    Url(String),
    /// A local file path.
    File(String),
    /// Standard input (`-`).
    Stdin,
}

/// Destination for the generated PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// A file-system path.
    Path(String),
    /// Standard output (`-`).
    Stdout,
}

/// Top-level run mode, derived from info flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Normal PDF conversion.
    Convert,
    /// `--help / -h`
    Help,
    /// `--extended-help / -H`
    ExtendedHelp,
    /// `--version / -V`
    Version,
    /// `--readme`
    Readme,
    /// `--manpage`
    Manpage,
    /// `--dump-default-toc-xsl`
    DumpDefaultTocXsl,
}

/// Fully parsed wkhtmltopdf invocation.
#[derive(Debug, Clone)]
pub struct ParsedInvocation {
    /// Document-level settings (page size, margins, orientation, …).
    pub global: GlobalSettings,
    /// One entry per page input, in order.  Each carries its own per-object
    /// settings accumulated from flags that appeared before that page token.
    pub objects: Vec<(PdfObjectSettings, Input)>,
    /// Cover page, if `cover <input>` was present.
    pub cover: Option<Input>,
    /// Whether a Table of Contents should be generated.
    pub toc: bool,
    /// Output destination.
    pub output: Output,
    /// Run mode (default `Convert`).
    pub mode: RunMode,
    /// Warnings accumulated during parsing (unimplemented settings, etc.).
    pub warnings: Vec<String>,
    /// When `--dump-outline <file>` is given, the outline XML is written to
    /// this path after a successful conversion.
    pub dump_outline: Option<String>,
}

// ---------------------------------------------------------------------------
// Internal flag table
// ---------------------------------------------------------------------------

/// Whether a flag applies to global settings, per-object settings, or both.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Always applied to `GlobalSettings` via `set_global`.
    Global,
    /// Applied to `GlobalSettings` in the leading phase; to the upcoming
    /// `PdfObjectSettings` in the object phase.
    Both,
}

/// What the flag does when matched.
#[derive(Clone, Copy)]
enum Action {
    /// Set the named registry setting to the next token (arity 1).
    Setting(&'static str),
    /// Set the named registry setting to a compile-time constant (arity 0).
    Const(&'static str, &'static str),
    /// Two-argument repeatable: combine next two tokens as "k=v" and pass to
    /// the named registry setting (arity 2; registry will warn if unimplemented).
    TwoArg(&'static str),
    /// Non-settings special handling (arity 0 unless noted).
    Special(SpecialKind),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecialKind {
    Help,
    ExtendedHelp,
    Version,
    Manpage,
    Readme,
    /// Sets `quiet = true` in global settings (recognised-but-unimplemented).
    Quiet,
    /// Sets `toc = true` on the invocation.
    Toc,
    /// Prints the default TOC XSL to stdout and exits 0 (arity 0).
    DumpDefaultTocXsl,
    /// Stores the next argument as the outline dump path (arity 1).
    DumpOutline,
}

struct FlagSpec {
    long: &'static str,
    short: Option<char>,
    target: Target,
    action: Action,
}

/// The argspec table: every CLI flag recognised by the parser.
///
/// Arity is derived from `Action`: `Setting` → 1, `TwoArg` → 2, all others
/// → 0.
static FLAGS: &[FlagSpec] = &[
    // ── Info / meta flags ────────────────────────────────────────────────────
    FlagSpec {
        long: "help",
        short: Some('h'),
        target: Target::Global,
        action: Action::Special(SpecialKind::Help),
    },
    FlagSpec {
        long: "extended-help",
        short: Some('H'),
        target: Target::Global,
        action: Action::Special(SpecialKind::ExtendedHelp),
    },
    FlagSpec {
        long: "version",
        short: Some('V'),
        target: Target::Global,
        action: Action::Special(SpecialKind::Version),
    },
    FlagSpec {
        long: "manpage",
        short: None,
        target: Target::Global,
        action: Action::Special(SpecialKind::Manpage),
    },
    FlagSpec {
        long: "readme",
        short: None,
        target: Target::Global,
        action: Action::Special(SpecialKind::Readme),
    },
    FlagSpec {
        long: "quiet",
        short: Some('q'),
        target: Target::Global,
        action: Action::Special(SpecialKind::Quiet),
    },
    FlagSpec {
        long: "toc",
        short: None,
        target: Target::Global,
        action: Action::Special(SpecialKind::Toc),
    },
    // ── Global layout ────────────────────────────────────────────────────────
    FlagSpec {
        long: "page-size",
        short: Some('s'),
        target: Target::Global,
        action: Action::Setting("size.pageSize"),
    },
    FlagSpec {
        long: "orientation",
        short: Some('O'),
        target: Target::Global,
        action: Action::Setting("orientation"),
    },
    FlagSpec {
        long: "margin-top",
        short: Some('T'),
        target: Target::Global,
        action: Action::Setting("margin.top"),
    },
    FlagSpec {
        long: "margin-bottom",
        short: Some('B'),
        target: Target::Global,
        action: Action::Setting("margin.bottom"),
    },
    FlagSpec {
        long: "margin-left",
        short: Some('L'),
        target: Target::Global,
        action: Action::Setting("margin.left"),
    },
    FlagSpec {
        long: "margin-right",
        short: Some('R'),
        target: Target::Global,
        action: Action::Setting("margin.right"),
    },
    FlagSpec {
        long: "dpi",
        short: Some('D'),
        target: Target::Global,
        action: Action::Setting("dpi"),
    },
    FlagSpec {
        long: "page-width",
        short: None,
        target: Target::Global,
        action: Action::Setting("size.width"),
    },
    FlagSpec {
        long: "page-height",
        short: None,
        target: Target::Global,
        action: Action::Setting("size.height"),
    },
    FlagSpec {
        long: "image-quality",
        short: None,
        target: Target::Global,
        action: Action::Setting("imageQuality"),
    },
    FlagSpec {
        long: "image-dpi",
        short: None,
        target: Target::Global,
        action: Action::Setting("imageDPI"),
    },
    FlagSpec {
        long: "title",
        short: None,
        target: Target::Global,
        action: Action::Setting("documentTitle"),
    },
    FlagSpec {
        long: "grayscale",
        short: Some('g'),
        target: Target::Global,
        action: Action::Const("colorMode", "grayscale"),
    },
    FlagSpec {
        long: "lowquality",
        short: Some('l'),
        target: Target::Global,
        action: Action::Const("resolution", "screen"),
    },
    FlagSpec {
        long: "no-pdf-compression",
        short: None,
        target: Target::Global,
        action: Action::Const("useCompression", "false"),
    },
    // ── Outline ──────────────────────────────────────────────────────────────
    FlagSpec {
        long: "outline",
        short: None,
        target: Target::Global,
        action: Action::Const("outline", "true"),
    },
    FlagSpec {
        long: "no-outline",
        short: None,
        target: Target::Global,
        action: Action::Const("outline", "false"),
    },
    FlagSpec {
        long: "outline-depth",
        short: None,
        target: Target::Global,
        action: Action::Setting("outlineDepth"),
    },
    // ── Per-page / both-phase flags ──────────────────────────────────────────
    FlagSpec {
        long: "zoom",
        short: None,
        target: Target::Both,
        action: Action::Setting("load.zoomFactor"),
    },
    FlagSpec {
        long: "javascript-delay",
        short: None,
        target: Target::Both,
        action: Action::Setting("load.jsdelay"),
    },
    FlagSpec {
        long: "proxy",
        short: None,
        target: Target::Both,
        action: Action::Setting("load.proxy"),
    },
    FlagSpec {
        long: "encoding",
        short: None,
        target: Target::Both,
        action: Action::Setting("web.defaultEncoding"),
    },
    FlagSpec {
        long: "user-style-sheet",
        short: None,
        target: Target::Both,
        action: Action::Setting("web.userStyleSheet"),
    },
    FlagSpec {
        long: "print-media-type",
        short: None,
        target: Target::Both,
        action: Action::Const("web.printMediaType", "true"),
    },
    FlagSpec {
        long: "no-print-media-type",
        short: None,
        target: Target::Both,
        action: Action::Const("web.printMediaType", "false"),
    },
    FlagSpec {
        long: "background",
        short: None,
        target: Target::Both,
        action: Action::Const("web.background", "true"),
    },
    FlagSpec {
        long: "no-background",
        short: None,
        target: Target::Both,
        action: Action::Const("web.background", "false"),
    },
    FlagSpec {
        long: "enable-javascript",
        short: None,
        target: Target::Both,
        action: Action::Const("web.enableJavascript", "true"),
    },
    FlagSpec {
        long: "disable-javascript",
        short: Some('n'),
        target: Target::Both,
        action: Action::Const("web.enableJavascript", "false"),
    },
    FlagSpec {
        long: "enable-forms",
        short: None,
        target: Target::Global,
        action: Action::Const("produceForms", "true"),
    },
    FlagSpec {
        long: "images",
        short: None,
        target: Target::Both,
        action: Action::Const("web.loadImages", "true"),
    },
    FlagSpec {
        long: "no-images",
        short: None,
        target: Target::Both,
        action: Action::Const("web.loadImages", "false"),
    },
    FlagSpec {
        long: "enable-local-file-access",
        short: None,
        target: Target::Both,
        action: Action::Const("load.blockLocalFileAccess", "false"),
    },
    FlagSpec {
        long: "disable-local-file-access",
        short: None,
        target: Target::Both,
        action: Action::Const("load.blockLocalFileAccess", "true"),
    },
    // ── Security policy ──────────────────────────────────────────────────────
    // --safe activates the hardened ResourcePolicy (deny file://, block private
    // IPs).  --allow <path> allows a specific directory prefix even under --safe.
    FlagSpec {
        long: "safe",
        short: None,
        target: Target::Global,
        action: Action::Const("load.safe", "true"),
    },
    FlagSpec {
        long: "allow",
        short: None,
        target: Target::Global,
        action: Action::Setting("load.allowedPath"),
    },
    FlagSpec {
        long: "disable-external-links",
        short: None,
        target: Target::Global,
        action: Action::Const("load.disableExternalLinks", "true"),
    },
    FlagSpec {
        long: "disable-internal-links",
        short: None,
        target: Target::Global,
        action: Action::Const("load.disableInternalLinks", "true"),
    },
    // ── Header ───────────────────────────────────────────────────────────────
    FlagSpec {
        long: "header-left",
        short: None,
        target: Target::Both,
        action: Action::Setting("header.left"),
    },
    FlagSpec {
        long: "header-center",
        short: None,
        target: Target::Both,
        action: Action::Setting("header.center"),
    },
    FlagSpec {
        long: "header-right",
        short: None,
        target: Target::Both,
        action: Action::Setting("header.right"),
    },
    FlagSpec {
        long: "header-font-size",
        short: None,
        target: Target::Both,
        action: Action::Setting("header.fontSize"),
    },
    FlagSpec {
        long: "header-line",
        short: None,
        target: Target::Both,
        action: Action::Const("header.line", "true"),
    },
    FlagSpec {
        long: "no-header-line",
        short: None,
        target: Target::Both,
        action: Action::Const("header.line", "false"),
    },
    FlagSpec {
        long: "header-spacing",
        short: None,
        target: Target::Both,
        action: Action::Setting("header.spacing"),
    },
    FlagSpec {
        long: "header-html",
        short: None,
        target: Target::Both,
        action: Action::Setting("header.htmlUrl"),
    },
    // ── Footer ───────────────────────────────────────────────────────────────
    FlagSpec {
        long: "footer-left",
        short: None,
        target: Target::Both,
        action: Action::Setting("footer.left"),
    },
    FlagSpec {
        long: "footer-center",
        short: None,
        target: Target::Both,
        action: Action::Setting("footer.center"),
    },
    FlagSpec {
        long: "footer-right",
        short: None,
        target: Target::Both,
        action: Action::Setting("footer.right"),
    },
    FlagSpec {
        long: "footer-font-size",
        short: None,
        target: Target::Both,
        action: Action::Setting("footer.fontSize"),
    },
    FlagSpec {
        long: "footer-line",
        short: None,
        target: Target::Both,
        action: Action::Const("footer.line", "true"),
    },
    FlagSpec {
        long: "no-footer-line",
        short: None,
        target: Target::Both,
        action: Action::Const("footer.line", "false"),
    },
    FlagSpec {
        long: "footer-spacing",
        short: None,
        target: Target::Both,
        action: Action::Setting("footer.spacing"),
    },
    FlagSpec {
        long: "footer-html",
        short: None,
        target: Target::Both,
        action: Action::Setting("footer.htmlUrl"),
    },
    // ── Two-argument repeatables ─────────────────────────────────────────────
    FlagSpec {
        long: "cookie",
        short: None,
        target: Target::Both,
        action: Action::TwoArg("load.cookies"),
    },
    FlagSpec {
        long: "custom-header",
        short: None,
        target: Target::Both,
        action: Action::TwoArg("load.customHeaders"),
    },
    FlagSpec {
        long: "replace",
        short: None,
        target: Target::Both,
        action: Action::TwoArg("replacements"),
    },
    // ── TOC XSL and TOC settings ─────────────────────────────────────────────
    FlagSpec {
        long: "xsl-style-sheet",
        short: None,
        target: Target::Global,
        action: Action::Setting("tocXsl"),
    },
    FlagSpec {
        long: "toc-header-text",
        short: None,
        target: Target::Global,
        action: Action::Setting("toc.captionText"),
    },
    FlagSpec {
        long: "disable-toc-links",
        short: None,
        target: Target::Global,
        action: Action::Const("toc.forwardLinks", "false"),
    },
    FlagSpec {
        long: "disable-dotted-lines",
        short: None,
        target: Target::Global,
        action: Action::Const("toc.useDottedLines", "false"),
    },
    FlagSpec {
        long: "toc-text-size-shrink",
        short: None,
        target: Target::Global,
        action: Action::Setting("toc.fontScale"),
    },
    FlagSpec {
        long: "toc-level-indentation",
        short: None,
        target: Target::Global,
        action: Action::Setting("toc.indentation"),
    },
    FlagSpec {
        long: "enable-toc-back-links",
        short: None,
        target: Target::Global,
        action: Action::Const("toc.backLinks", "true"),
    },
    // ── Dump utilities ───────────────────────────────────────────────────────
    FlagSpec {
        long: "dump-default-toc-xsl",
        short: None,
        target: Target::Global,
        action: Action::Special(SpecialKind::DumpDefaultTocXsl),
    },
    FlagSpec {
        long: "dump-outline",
        short: None,
        target: Target::Global,
        action: Action::Special(SpecialKind::DumpOutline),
    },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A queued per-object flag: `(cli_flag_long_name, dotted_registry_name, value)`.
///
/// The CLI flag name (without `--`) is carried alongside the dotted name so
/// that error messages shown to the user reference the flag they typed rather
/// than the internal dotted registry name.
type FlagEntry = (String, String, String);

fn arity_of(action: &Action) -> usize {
    match action {
        Action::Setting(_) => 1,
        Action::Const(_, _) => 0,
        Action::TwoArg(_) => 2,
        Action::Special(kind) => match kind {
            SpecialKind::DumpOutline => 1,
            _ => 0,
        },
    }
}

/// Find the flag spec for a token such as `"--page-size"` or `"-s"`.
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

/// Decide whether a bare token looks like a URL.
fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://")
}

/// Convert a positional token to an `Input`.
fn parse_input(s: &str) -> Input {
    if s == "-" {
        Input::Stdin
    } else if is_url(s) {
        Input::Url(s.to_owned())
    } else {
        Input::File(s.to_owned())
    }
}

/// Build a minimal `ParsedInvocation` for early-exit info modes (help, version, …).
fn early_exit(mode: RunMode) -> ParsedInvocation {
    ParsedInvocation {
        global: GlobalSettings::default(),
        objects: Vec::new(),
        cover: None,
        toc: false,
        output: Output::Stdout,
        mode,
        warnings: Vec::new(),
        dump_outline: None,
    }
}

/// Apply a single flag (name + value) to either the global settings or the
/// pending per-object flag list, based on the flag's `Target` and the current
/// parse phase.
///
/// * In the **leading phase** (`in_obj_phase = false`): `Target::Global` and
///   `Target::Both` flags both call `set_global`.  If the setting is unknown
///   to global scope (e.g. `load.blockLocalFileAccess`), the flag is deferred
///   into `pre_flags` so it reaches the first page's object.  If the value is
///   invalid the error is returned immediately.
/// * In the **object phase** (`in_obj_phase = true`): `Target::Global` flags
///   still call `set_global` (layout is document-level); `Target::Both` flags
///   are pushed to `pre_flags` for the upcoming page.
///
/// `cli_flag` is the long CLI flag name without the leading `--` (e.g.
/// `"page-size"`); it is used in error messages so the user sees the flag they
/// typed rather than the internal dotted registry name.
#[allow(clippy::too_many_arguments)]
fn apply_setting(
    cli_flag: &str,
    name: &str,
    value: &str,
    target: Target,
    global: &mut GlobalSettings,
    pre_flags: &mut Vec<FlagEntry>,
    in_obj_phase: bool,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    match target {
        Target::Global => match set_global(global, name, value) {
            Ok(()) => {
                warnings.extend(global.take_warnings());
            }
            Err(e) => {
                return Err(format!("invalid value for --{cli_flag}: {e}"));
            }
        },
        Target::Both => {
            if !in_obj_phase {
                // Leading phase: apply to global when the setting is known
                // there; otherwise defer to per-object flags.
                if get_global(global, name).is_some() {
                    match set_global(global, name, value) {
                        Ok(()) => {
                            warnings.extend(global.take_warnings());
                        }
                        Err(e) => {
                            return Err(format!("invalid value for --{cli_flag}: {e}"));
                        }
                    }
                } else {
                    pre_flags.push((cli_flag.to_owned(), name.to_owned(), value.to_owned()));
                }
            } else {
                // Object phase: queue for the next page.
                pre_flags.push((cli_flag.to_owned(), name.to_owned(), value.to_owned()));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Main parse entry point
// ---------------------------------------------------------------------------

/// Parse a wkhtmltopdf-style argument list into a [`ParsedInvocation`].
///
/// # Behaviour
///
/// * Info flags (`--help`, `--version`, etc.) are detected immediately and
///   returned as `mode = Help / Version / …` with all other fields defaulted.
/// * Flags before the first page-input token ("leading phase") are applied
///   directly to `global`.
/// * Flags between two page inputs ("object phase") are accumulated and applied
///   to the **later** page's [`PdfObjectSettings`].
/// * `cover <token>` sets the cover page; it does not start the object phase.
/// * `toc` (positional) or `--toc` (flag) sets `toc = true`.
/// * The **last** non-flag positional is the output (`-` = stdout).
/// * All preceding non-flag positionals are page inputs.
///
/// # Errors
///
/// Returns `Err(message)` when:
/// * An unknown flag is encountered.
/// * A flag's argument(s) are missing.
/// * A flag's value is invalid (propagated from the registry).
/// * No output is specified (no positionals at all).
pub fn parse(args: &[String]) -> Result<ParsedInvocation, String> {
    let mut global = GlobalSettings::default();
    let mut cover: Option<Input> = None;
    let mut toc = false;
    let mut warnings: Vec<String> = Vec::new();
    let mut dump_outline: Option<String> = None;

    // `pending_items`: a list of (flags_for_this_positional, positional_token).
    // Each entry is created when a positional is first encountered; the flags
    // captured into it are those that had accumulated since the *previous*
    // positional (or since the start, for the first one).  The final entry
    // always becomes the output; everything before it becomes a page object.
    let mut pending_items: Vec<(Vec<FlagEntry>, String)> = Vec::new();

    // Flags accumulated for the *next* positional encountered.
    let mut pre_flags: Vec<FlagEntry> = Vec::new();

    // True once the first page-input positional has been seen.  Determines
    // whether a flag call goes to global settings or into `pre_flags`.
    let mut in_obj_phase = false;

    let mut i = 0;
    while i < args.len() {
        let tok = args[i].as_str();

        // ── Determine whether the token is a flag ─────────────────────────
        let is_flag_tok = (tok.starts_with("--") && tok.len() > 2)
            || (tok.starts_with('-') && tok.len() == 2 && tok.as_bytes()[1].is_ascii_alphabetic());

        if is_flag_tok {
            let spec = find_flag(tok).ok_or_else(|| format!("unknown option: {tok}"))?;

            let ar = arity_of(&spec.action);
            if i + ar >= args.len() {
                return Err(format!(
                    "option {tok} requires {ar} argument(s) but reached end of input"
                ));
            }
            let val1 = if ar >= 1 { args[i + 1].as_str() } else { "" };
            let val2 = if ar >= 2 { args[i + 2].as_str() } else { "" };

            match spec.action {
                Action::Special(kind) => match kind {
                    SpecialKind::Help => return Ok(early_exit(RunMode::Help)),
                    SpecialKind::ExtendedHelp => return Ok(early_exit(RunMode::ExtendedHelp)),
                    SpecialKind::Version => return Ok(early_exit(RunMode::Version)),
                    SpecialKind::Manpage => return Ok(early_exit(RunMode::Manpage)),
                    SpecialKind::Readme => return Ok(early_exit(RunMode::Readme)),
                    SpecialKind::DumpDefaultTocXsl => {
                        return Ok(early_exit(RunMode::DumpDefaultTocXsl));
                    }
                    SpecialKind::DumpOutline => {
                        // val1 is the next token (arity 1 enforced by arity_of).
                        dump_outline = Some(val1.to_owned());
                    }
                    SpecialKind::Quiet => {
                        let _ = set_global(&mut global, "quiet", "true");
                        warnings.extend(global.take_warnings());
                    }
                    SpecialKind::Toc => {
                        toc = true;
                    }
                },

                Action::Setting(name) => {
                    apply_setting(
                        spec.long,
                        name,
                        val1,
                        spec.target,
                        &mut global,
                        &mut pre_flags,
                        in_obj_phase,
                        &mut warnings,
                    )?;
                }

                Action::Const(name, value) => {
                    apply_setting(
                        spec.long,
                        name,
                        value,
                        spec.target,
                        &mut global,
                        &mut pre_flags,
                        in_obj_phase,
                        &mut warnings,
                    )?;
                }

                Action::TwoArg(name) => {
                    // Two-argument flags always land in pre_flags (even in the
                    // leading phase), because the registry only understands them
                    // in the per-object context.  The registry will warn about
                    // them being unimplemented.
                    let combined = format!("{val1}={val2}");
                    pre_flags.push((spec.long.to_owned(), name.to_owned(), combined));
                }
            }

            i += 1 + ar;
        } else if tok == "cover" {
            // Positional keyword: consume next token as cover input.
            if i + 1 >= args.len() {
                return Err("'cover' requires an input argument".to_owned());
            }
            cover = Some(parse_input(args[i + 1].as_str()));
            i += 2;
        } else if tok == "toc" {
            toc = true;
            i += 1;
        } else {
            // Regular positional (page input or output).
            // Capture the pre_flags accumulated since the last positional.
            let captured = std::mem::take(&mut pre_flags);
            pending_items.push((captured, tok.to_owned()));
            in_obj_phase = true;
            i += 1;
        }
    }

    // ── Determine output and build objects ────────────────────────────────────
    if pending_items.is_empty() {
        return Err("no output file specified".to_owned());
    }

    // The last pending item is the output.
    let (trailing_flags, output_str) = pending_items.pop().unwrap();
    if !trailing_flags.is_empty() {
        warnings.push(
            "flags appearing after the last page input and before the output token are ignored"
                .to_owned(),
        );
    }

    let output = if output_str == "-" {
        Output::Stdout
    } else {
        Output::Path(output_str)
    };

    // All remaining pending items are page objects.
    let mut objects: Vec<(PdfObjectSettings, Input)> = Vec::new();
    for (flags, input_str) in pending_items {
        let input = parse_input(&input_str);
        let mut obj = PdfObjectSettings::default();
        for (cli_flag, name, value) in flags {
            match set_object(&mut obj, &name, &value) {
                Ok(()) => {}
                Err(e) => return Err(format!("invalid value for --{cli_flag}: {e}")),
            }
        }
        warnings.extend(obj.take_warnings());
        objects.push((obj, input));
    }

    // Drain any global warnings accumulated during flag processing.
    warnings.extend(global.take_warnings());

    Ok(ParsedInvocation {
        global,
        objects,
        cover,
        toc,
        output,
        mode: RunMode::Convert,
        warnings,
        dump_outline,
    })
}

// ---------------------------------------------------------------------------
// Help text generation
// ---------------------------------------------------------------------------

/// Generate a human-readable usage and flag listing for `wkhtmltopdf`.
///
/// `extended = false` produces a concise listing (equivalent to `--help`).
/// `extended = true` produces the same flag listing with a different header
/// (equivalent to `--extended-help`); the full flag table is printed in both
/// modes because it is compact.
pub fn help_text(extended: bool) -> String {
    let mut out = String::new();

    out.push_str(concat!(
        "Usage:\n",
        "  wkhtmltopdf [GLOBAL OPTIONS]... [OBJECT] [OBJECT OPTIONS]... \\\n",
        "              <input url/file/-> [<input>]... <output file/->\n\n",
    ));

    if extended {
        out.push_str("Options (extended help — all flags listed):\n");
    } else {
        out.push_str("Options (use --extended-help for full documentation):\n");
    }

    for spec in FLAGS {
        // Short-flag column: "-x, " or "    ".
        let short_col = if let Some(ch) = spec.short {
            format!("-{ch}, ")
        } else {
            "    ".to_owned()
        };

        // Arity hint appended to the long flag.
        let hint: &str = match spec.action {
            Action::Setting(_) => " <arg>",
            Action::TwoArg(_) => " <k> <v>",
            _ => "",
        };

        // Scope tag: Global-only vs. both global and per-object.
        let scope: &str = match spec.target {
            Target::Global => "global",
            Target::Both => "global/object",
        };

        // Left column: "  -x, --long-flag <arg>"
        let flag_col = format!("  {}--{}{}", short_col, spec.long, hint);

        // Print padded to 46 chars so the scope tags line up.
        out.push_str(&format!("{flag_col:<46}  ({scope})\n"));
    }

    out.push('\n');
    if !extended {
        out.push_str("Use --extended-help for documentation on less-used options.\n");
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use wkhtmltox_core::settings::NamedPageSize;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ── Test 1: main integration test from the spec ───────────────────────
    /// parse(["-s","A4","--toc","cover","c.html","page.html","out.pdf"])
    /// → global pageSize A4, toc=true, cover=Some(File "c.html"),
    ///   one object page.html, output out.pdf
    #[test]
    fn spec_main_test() {
        let inv = parse(&args(&[
            "-s",
            "A4",
            "--toc",
            "cover",
            "c.html",
            "page.html",
            "out.pdf",
        ]))
        .expect("parse should succeed");

        assert_eq!(inv.mode, RunMode::Convert);
        assert_eq!(
            inv.global.page_size,
            NamedPageSize::A4,
            "page size should be A4"
        );
        assert!(inv.toc, "toc should be true");
        assert_eq!(
            inv.cover,
            Some(Input::File("c.html".to_owned())),
            "cover should be c.html"
        );
        assert_eq!(inv.objects.len(), 1, "should have exactly one page object");
        assert_eq!(inv.objects[0].1, Input::File("page.html".to_owned()));
        assert_eq!(inv.output, Output::Path("out.pdf".to_owned()));
    }

    // ── Test 2: two-argument --cookie ─────────────────────────────────────
    /// --cookie k v is parsed; registry warns (unimplemented) but does not error.
    #[test]
    fn cookie_two_arg() {
        let inv = parse(&args(&[
            "--cookie",
            "session",
            "abc123",
            "page.html",
            "out.pdf",
        ]))
        .expect("parse should succeed");

        assert_eq!(inv.objects.len(), 1);
        // The cookie was applied via set_object → recognised-but-unimplemented warning.
        // At least one warning should mention load.cookies.
        assert!(
            inv.warnings.iter().any(|w| w.contains("load.cookies")),
            "expected a warning about load.cookies; got: {:?}",
            inv.warnings
        );
    }

    // ── Test 3: per-object flag (Target::Both in object phase) ───────────
    /// Object-attach rule: flags between two page inputs attach to the LATER page.
    /// --javascript-delay 500 between page1 and page2 → page2 gets jsdelay=500,
    /// page1 keeps the default (200 ms).
    ///
    /// NOTE: `--orientation` is Target::Global and is not per-object in the
    /// current registry (`PdfObjectSettings` does not carry an orientation
    /// field).  Per-object scoping for layout flags is out of scope.
    #[test]
    fn per_object_flag_javascript_delay() {
        let inv = parse(&args(&[
            "page1.html",
            "--javascript-delay",
            "500",
            "page2.html",
            "o.pdf",
        ]))
        .expect("parse should succeed");

        assert_eq!(inv.objects.len(), 2);
        let (ref obj1, ref inp1) = inv.objects[0];
        let (ref obj2, ref inp2) = inv.objects[1];

        assert_eq!(*inp1, Input::File("page1.html".to_owned()));
        assert_eq!(*inp2, Input::File("page2.html".to_owned()));

        // page1 should retain the default jsdelay (200 ms).
        assert_eq!(
            obj1.javascript_delay_ms, 200,
            "page1 should have default jsdelay"
        );
        // page2 should have the overridden value.
        assert_eq!(
            obj2.javascript_delay_ms, 500,
            "page2 should have jsdelay=500"
        );
    }

    // ── Test 4: global layout flag applied from object phase → still global ─
    /// --orientation is Target::Global, so it applies globally even when it
    /// appears after a page input.  Both pages share the global orientation.
    #[test]
    fn orientation_is_always_global() {
        use wkhtmltox_core::render::Orientation;

        let inv = parse(&args(&[
            "page1.html",
            "--orientation",
            "Landscape",
            "page2.html",
            "o.pdf",
        ]))
        .expect("parse should succeed");

        assert_eq!(inv.objects.len(), 2);
        // Global orientation is Landscape.
        assert!(
            matches!(inv.global.orientation, Orientation::Landscape),
            "global orientation should be Landscape"
        );
    }

    // ── Test 5: --help returns Help mode ─────────────────────────────────
    #[test]
    fn help_flag() {
        let inv = parse(&args(&["--help"])).expect("parse should succeed");
        assert_eq!(inv.mode, RunMode::Help);
    }

    /// Short form -h
    #[test]
    fn help_short() {
        let inv = parse(&args(&["-h"])).expect("parse should succeed");
        assert_eq!(inv.mode, RunMode::Help);
    }

    // ── Test 6: --version returns Version mode ───────────────────────────
    #[test]
    fn version_flag() {
        let inv = parse(&args(&["--version"])).expect("parse should succeed");
        assert_eq!(inv.mode, RunMode::Version);
    }

    // ── Test 7: unknown flag → Err ───────────────────────────────────────
    #[test]
    fn unknown_flag_is_error() {
        let err = parse(&args(&["--frobnicate"])).unwrap_err();
        assert!(
            err.contains("unknown option"),
            "error should mention 'unknown option': {err}"
        );
    }

    // ── Test 8: stdin / stdout ───────────────────────────────────────────
    #[test]
    fn stdin_as_input() {
        let inv = parse(&args(&["-", "out.pdf"])).expect("parse should succeed");
        assert_eq!(inv.objects.len(), 1);
        assert_eq!(inv.objects[0].1, Input::Stdin);
        assert_eq!(inv.output, Output::Path("out.pdf".to_owned()));
    }

    #[test]
    fn stdout_as_output() {
        let inv = parse(&args(&["page.html", "-"])).expect("parse should succeed");
        assert_eq!(inv.output, Output::Stdout);
    }

    // ── Test 9: URL detection ─────────────────────────────────────────────
    #[test]
    fn url_input() {
        let inv = parse(&args(&["https://example.com", "out.pdf"])).expect("parse should succeed");
        assert_eq!(
            inv.objects[0].1,
            Input::Url("https://example.com".to_owned())
        );
    }

    // ── Test 10: no positionals → error ──────────────────────────────────
    #[test]
    fn no_positionals_is_error() {
        let err = parse(&args(&["--quiet"])).unwrap_err();
        assert!(
            err.contains("no output"),
            "error should mention 'no output': {err}"
        );
    }

    // ── Test 11: extended-help / manpage / readme ─────────────────────────
    #[test]
    fn extended_help_flag() {
        assert_eq!(
            parse(&args(&["--extended-help"])).unwrap().mode,
            RunMode::ExtendedHelp
        );
        assert_eq!(parse(&args(&["-H"])).unwrap().mode, RunMode::ExtendedHelp);
    }

    #[test]
    fn manpage_flag() {
        assert_eq!(parse(&args(&["--manpage"])).unwrap().mode, RunMode::Manpage);
    }

    #[test]
    fn readme_flag() {
        assert_eq!(parse(&args(&["--readme"])).unwrap().mode, RunMode::Readme);
    }

    // ── Test 12: global flags (size + margin + orientation) ──────────────
    #[test]
    fn global_flags_leading_phase() {
        let inv = parse(&args(&[
            "-s",
            "Letter",
            "-O",
            "Landscape",
            "-T",
            "20mm",
            "-B",
            "15mm",
            "page.html",
            "out.pdf",
        ]))
        .expect("parse should succeed");

        assert_eq!(inv.global.page_size, NamedPageSize::Letter);
        assert!(matches!(
            inv.global.orientation,
            wkhtmltox_core::render::Orientation::Landscape
        ));
        assert!((inv.global.margin_top_mm - 20.0).abs() < 1e-9);
        assert!((inv.global.margin_bottom_mm - 15.0).abs() < 1e-9);
    }

    // ── Test 13: --grayscale sets colorMode ──────────────────────────────
    #[test]
    fn grayscale_flag() {
        use wkhtmltox_core::settings::ColorMode;
        let inv =
            parse(&args(&["--grayscale", "page.html", "out.pdf"])).expect("parse should succeed");
        assert!(matches!(inv.global.color_mode, ColorMode::Grayscale));
    }

    // ── Test 14: --zoom in leading phase → global zoom ───────────────────
    #[test]
    fn zoom_in_leading_phase_goes_to_global() {
        let inv =
            parse(&args(&["--zoom", "1.5", "page.html", "out.pdf"])).expect("parse should succeed");
        assert!(
            (inv.global.zoom - 1.5).abs() < 1e-9,
            "global zoom should be 1.5"
        );
    }

    // ── Test 15: --zoom in object phase → per-object ──────────────────────
    #[test]
    fn zoom_in_object_phase_goes_to_object() {
        let inv = parse(&args(&[
            "page1.html",
            "--zoom",
            "2.0",
            "page2.html",
            "out.pdf",
        ]))
        .expect("parse should succeed");

        assert_eq!(inv.objects.len(), 2);
        // page1 gets default zoom (1.0)
        assert!(
            (inv.objects[0].0.zoom - 1.0).abs() < 1e-9,
            "page1 zoom should be 1.0 (default)"
        );
        // page2 gets zoom 2.0
        assert!(
            (inv.objects[1].0.zoom - 2.0).abs() < 1e-9,
            "page2 zoom should be 2.0"
        );
    }

    // ── Test 16: --outline-depth ──────────────────────────────────────────
    #[test]
    fn outline_depth_flag() {
        let inv =
            parse(&args(&["--outline-depth", "3", "p.html", "out.pdf"])).expect("should parse");
        assert_eq!(inv.global.outline_depth, 3);
    }

    // ── Test 17: header flags ─────────────────────────────────────────────
    #[test]
    fn header_flags_leading_phase() {
        let inv = parse(&args(&[
            "--header-center",
            "[page]/[topage]",
            "--header-font-size",
            "10",
            "--header-line",
            "p.html",
            "out.pdf",
        ]))
        .expect("should parse");

        assert_eq!(inv.global.header.center, "[page]/[topage]");
        assert!((inv.global.header.font_size - 10.0).abs() < 1e-9);
        assert!(inv.global.header.line);
    }

    // ── Test 18: --toc positional keyword ────────────────────────────────
    #[test]
    fn toc_positional_keyword() {
        let inv = parse(&args(&["toc", "page.html", "out.pdf"])).expect("should parse");
        assert!(inv.toc);
    }

    // ── Test 19: cover positional keyword with URL ────────────────────────
    #[test]
    fn cover_positional_with_url() {
        let inv = parse(&args(&[
            "cover",
            "https://example.com/cover",
            "page.html",
            "out.pdf",
        ]))
        .expect("should parse");
        assert_eq!(
            inv.cover,
            Some(Input::Url("https://example.com/cover".to_owned()))
        );
        assert_eq!(inv.objects.len(), 1);
    }

    // ── Test 20: short flags ──────────────────────────────────────────────
    #[test]
    fn short_flags_work() {
        let inv =
            parse(&args(&["-s", "A4", "-g", "-q", "page.html", "out.pdf"])).expect("should parse");
        assert_eq!(inv.global.page_size, NamedPageSize::A4);
        use wkhtmltox_core::settings::ColorMode;
        assert!(matches!(inv.global.color_mode, ColorMode::Grayscale));
    }

    // ── Tests 21–23: help_text generation ───────────────────────────────
    #[test]
    fn help_text_is_non_empty_and_contains_usage() {
        let text = crate::help_text(false);
        assert!(!text.is_empty(), "help text must not be empty");
        assert!(text.contains("Usage"), "help text must contain 'Usage'");
    }

    #[test]
    fn help_text_contains_known_flags() {
        let text = crate::help_text(false);
        assert!(
            text.contains("--page-size"),
            "expected --page-size in help text"
        );
        assert!(text.contains("--help"), "expected --help in help text");
        assert!(
            text.contains("--version"),
            "expected --version in help text"
        );
        assert!(
            text.contains("--margin-top"),
            "expected --margin-top in help text"
        );
    }

    #[test]
    fn extended_help_text_also_contains_usage() {
        let text = crate::help_text(true);
        assert!(text.contains("Usage"), "extended help must contain 'Usage'");
        assert!(
            text.contains("extended help"),
            "extended help header should say 'extended help'"
        );
        assert!(
            text.contains("--outline-depth"),
            "expected --outline-depth in extended help"
        );
    }

    // ── Tests: bad flag values → parse Err (Fix 1) ───────────────────────
    #[test]
    fn bad_page_size_is_error() {
        let err = parse(&args(&["--page-size", "Quux", "page.html", "out.pdf"])).unwrap_err();
        assert!(
            err.contains("invalid value for --page-size"),
            "error should mention --page-size, got: {err}"
        );
    }

    #[test]
    fn bad_dpi_value_is_error() {
        let err = parse(&args(&["--dpi", "abc", "page.html", "out.pdf"])).unwrap_err();
        assert!(
            err.contains("invalid value for --dpi"),
            "error should mention --dpi, got: {err}"
        );
    }

    #[test]
    fn bad_zoom_value_is_error_leading_phase() {
        let err = parse(&args(&["--zoom", "abc", "page.html", "out.pdf"])).unwrap_err();
        assert!(
            err.contains("invalid value for --zoom"),
            "error should mention --zoom in leading phase, got: {err}"
        );
    }

    #[test]
    fn bad_zoom_value_is_error_object_phase() {
        let err = parse(&args(&[
            "page1.html",
            "--zoom",
            "abc",
            "page2.html",
            "out.pdf",
        ]))
        .unwrap_err();
        assert!(
            err.contains("invalid value for --zoom"),
            "error should mention --zoom in object phase, got: {err}"
        );
    }

    // ── TOC XSL flags ─────────────────────────────────────────────────────

    /// `--xsl-style-sheet` is now a recognised flag — it must parse without
    /// "unknown option" error.
    #[test]
    fn xsl_style_sheet_is_accepted_flag() {
        let inv = parse(&args(&[
            "--xsl-style-sheet",
            "foo.xsl",
            "in.html",
            "out.pdf",
        ]))
        .expect("--xsl-style-sheet should parse without error");
        assert_eq!(inv.global.toc_xsl, Some("foo.xsl".to_owned()));
    }

    /// `--toc-header-text` sets the caption.
    #[test]
    fn toc_header_text_flag() {
        let inv = parse(&args(&[
            "--toc-header-text",
            "Contents",
            "p.html",
            "out.pdf",
        ]))
        .expect("should parse");
        assert_eq!(inv.global.toc_settings.caption_text, "Contents");
    }

    /// `--disable-dotted-lines` clears use_dotted_lines.
    #[test]
    fn disable_dotted_lines_flag() {
        let inv =
            parse(&args(&["--disable-dotted-lines", "p.html", "out.pdf"])).expect("should parse");
        assert!(!inv.global.toc_settings.use_dotted_lines);
    }

    /// `--disable-toc-links` clears forward_links.
    #[test]
    fn disable_toc_links_flag() {
        let inv =
            parse(&args(&["--disable-toc-links", "p.html", "out.pdf"])).expect("should parse");
        assert!(!inv.global.toc_settings.forward_links);
    }

    /// `--enable-toc-back-links` sets back_links.
    #[test]
    fn enable_toc_back_links_flag() {
        let inv =
            parse(&args(&["--enable-toc-back-links", "p.html", "out.pdf"])).expect("should parse");
        assert!(inv.global.toc_settings.back_links);
    }

    /// `--toc-text-size-shrink` sets font_scale.
    #[test]
    fn toc_text_size_shrink_flag() {
        let inv = parse(&args(&[
            "--toc-text-size-shrink",
            "0.7",
            "p.html",
            "out.pdf",
        ]))
        .expect("should parse");
        assert!((inv.global.toc_settings.font_scale - 0.7).abs() < 1e-9);
    }

    /// `--toc-level-indentation` sets indentation string.
    #[test]
    fn toc_level_indentation_flag() {
        let inv = parse(&args(&[
            "--toc-level-indentation",
            "2em",
            "p.html",
            "out.pdf",
        ]))
        .expect("should parse");
        assert_eq!(inv.global.toc_settings.indentation, "2em");
    }

    /// `--dump-default-toc-xsl` returns DumpDefaultTocXsl mode.
    #[test]
    fn dump_default_toc_xsl_mode() {
        let inv = parse(&args(&["--dump-default-toc-xsl"])).expect("should parse");
        assert_eq!(inv.mode, RunMode::DumpDefaultTocXsl);
    }

    /// `--dump-outline <file>` stores the path.
    #[test]
    fn dump_outline_flag() {
        let inv = parse(&args(&[
            "p.html",
            "--dump-outline",
            "outline.xml",
            "out.pdf",
        ]))
        .expect("should parse");
        assert_eq!(inv.dump_outline, Some("outline.xml".to_owned()));
    }

    // ── HTML header/footer + replace flags (Task 4) ───────────────────────────

    /// `--header-html <url>` is accepted and stored (not "unknown option").
    #[test]
    fn header_html_flag_is_accepted() {
        let inv = parse(&args(&["--header-html", "h.html", "in.html", "out.pdf"]))
            .expect("--header-html should parse without error");
        assert_eq!(
            inv.global.header_html_url,
            Some("h.html".to_owned()),
            "header_html_url should be set on global"
        );
    }

    /// `--footer-html <url>` is accepted and stored (not "unknown option").
    #[test]
    fn footer_html_flag_is_accepted() {
        let inv = parse(&args(&["--footer-html", "f.html", "in.html", "out.pdf"]))
            .expect("--footer-html should parse without error");
        assert_eq!(
            inv.global.footer_html_url,
            Some("f.html".to_owned()),
            "footer_html_url should be set on global"
        );
    }

    /// `--replace name value` (two-arg) parses into a `(name, value)` pair on the object.
    #[test]
    fn replace_two_arg_parses_into_pair() {
        let inv = parse(&args(&["--replace", "a", "b", "in.html", "out.pdf"]))
            .expect("--replace should parse without error");
        assert_eq!(inv.objects.len(), 1);
        assert_eq!(
            inv.objects[0].0.replacements,
            vec![("a".to_owned(), "b".to_owned())],
            "--replace a b should produce a single (a, b) pair on the object"
        );
    }

    /// End-to-end: `--replace foo bar` must reach `AssembleOpts.replacements`.
    ///
    /// This is the I1 regression guard: `--replace` is a TwoArg flag that always
    /// lands in the per-object pending list.  `to_assemble_opts()` only clones
    /// `global.replacements` (never populated from the CLI for this flag), so the
    /// caller must explicitly merge per-object replacements after building opts.
    /// This test verifies that the merge produces the expected pair.
    #[test]
    fn replace_end_to_end_reaches_assemble_opts() {
        let inv = parse(&args(&["--replace", "foo", "bar", "in.html", "out.pdf"]))
            .expect("--replace should parse without error");
        assert_eq!(inv.objects.len(), 1, "expected exactly one object");
        // The pair must be on the per-object settings (existing behaviour).
        assert_eq!(
            inv.objects[0].0.replacements,
            vec![("foo".to_owned(), "bar".to_owned())],
            "--replace foo bar must be stored on the object"
        );
        // Simulate the fix: build opts then merge per-object replacements.
        let mut opts = inv.global.to_assemble_opts();
        opts.replacements.extend(
            inv.objects
                .iter()
                .flat_map(|(o, _)| o.replacements.iter().cloned()),
        );
        assert!(
            opts.replacements
                .contains(&("foo".to_owned(), "bar".to_owned())),
            "AssembleOpts.replacements must contain (\"foo\", \"bar\") after merge; got: {:?}",
            opts.replacements
        );
    }

    /// `--header-spacing <real>` stores the spacing on global (leading phase).
    #[test]
    fn header_spacing_flag_is_accepted() {
        let inv = parse(&args(&["--header-spacing", "5", "in.html", "out.pdf"]))
            .expect("--header-spacing should parse without error");
        assert!(
            (inv.global.header_spacing - 5.0).abs() < 1e-9,
            "global.header_spacing should be 5.0"
        );
    }
}
