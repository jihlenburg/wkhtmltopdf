# wkhtmltox-rs Milestone 3b — wkhtmltopdf CLI

> SDD execution. Goal: a `wkhtmltopdf` executable with a wkhtmltopdf-compatible flag grammar, routing through the settings registry → assembly core. (`wkhtmltoimage` CLI lands with M5's image pipeline.)

## Global Constraints
- Rust 2021; no Qt. The CLI crate may use `unsafe`-free code only (no `forbid` needed but keep it clean). LGPL headers; `cargo clippy -D warnings` clean. Commit per task + trailer.
- Flag NAMES and semantics mirror upstream `wkhtmltopdf --extended-help` / `src/pdf/pdfarguments.cc` / `src/shared/commonarguments.cc`. Map each CLI flag → a registry setting name (dotted) so the CLI and C ABI share one settings path.
- Exit codes derived from the reference binary where feasible (0 success).

### Task 1 — Arg model + page-object grammar parser (pure, tested)
- New crate `wkhtmltox-cli` (lib): `pub struct ParsedInvocation { global: GlobalSettings, objects: Vec<(PdfObjectSettings, Input)>, cover: Option<Input>, toc: bool, output: Output, mode: RunMode }` where `Input = Url(String)|Stdin|File(String)`, `Output = Path(String)|Stdout`, `RunMode = Convert|Help|ExtendedHelp|Version|Readme|Manpage`.
- An **argspec table**: each entry = (long flag, optional short, arity 0/1/2, target = Global|Object, setting-name-or-handler). Cover the common flags: `--page-size/-s`, `--orientation/-O`, `--margin-top/-T`/`-B`/`-L`/`-R`, `--dpi/-D`, `--zoom`, `--grayscale/-g`, `--lowquality`, `--title`, `--no-pdf-compression`, `--enable-local-file-access`/`--disable-local-file-access`, `--javascript-delay`, `--no-images`, `--encoding`, `--user-style-sheet`, header/footer (`--header-left/center/right/-spacing/-font-size/-line`, footer-*), `--toc`, `--outline`/`--no-outline`, `--outline-depth`, `--print-media-type`/`--no-print-media-type`, two-arg repeatables `--cookie`/`--custom-header`/`--replace`, plus `--help/-h`, `--extended-help/-H`, `--version/-V`, `--readme`, `--manpage`, `--quiet/-q`. Unknown flag → error (exit 1, message like upstream).
- Parser: walk argv. Leading flags (before any input/subcommand) set GLOBAL; `cover <input>` and `toc` are positional pseudo-objects; flags appearing before an input attach to THAT object; the final positional = output (or `-`=stdout); `-` as input = stdin. Map each flag via the argspec → `registry::set_global`/`set_object` (translating CLI name → dotted setting name); collect warnings.
- Tests (pure, no browser): parse `["-s","A4","--toc","cover","c.html","page.html","out.pdf"]` → global pageSize A4, toc=true, cover=c.html, one object page.html, output out.pdf. Parse two-arg `--cookie k v`. Parse `--help`→RunMode::Help. Unknown flag→Err. Stdin/stdout (`-`).
- Commit `feat(cli): arg model + page-object grammar parser + flag→setting mapping`.

### Task 2 — `wkhtmltopdf` executable
- New crate `wkhtmltopdf-cli` (bin `wkhtmltopdf`) depending on `wkhtmltox-cli` + core + render-chromium. `main()`:
  - Parse argv → `ParsedInvocation`. Handle `--version`/`--help`/`--extended-help`/`--readme`/`--manpage` (generate help text from the argspec table) and exit 0.
  - For Convert: read stdin if any input is `-` (write to a temp file → file:// URL); build `Vec<Source>` + `AssembleOpts` (toc/cover/header/footer/number from settings); spawn `ChromiumRenderer`; `assemble_pdf` to the output path (or stdout = write bytes to stdout if `-`). Print progress to stderr unless `--quiet`.
  - Exit codes: 0 success; nonzero on parse error / load failure / io error (pick a small scheme; refine against the oracle in T3).
- Test: a gated (Chrome) integration test running the built binary on a small HTML file → output is a %PDF; `--version` prints; unknown flag → nonzero exit. Unit-test help-text generation.
- Commit `feat(cli): wkhtmltopdf executable (convert + help/version + stdin/stdout + exit codes)`.

### Task 3 — Oracle CLI comparison + milestone gate
- Harness: run OUR `wkhtmltopdf` and the oracle on the same args/doc (`-s A4 --toc page1 page2 out.pdf`), compare structurally (page count ±tol, outline titles, TOC present) + compare a few exit codes (success, unknown-flag, missing-input). Record `results-cli.md`.
- Then whole-milestone Opus review + security audit (CLI parses untrusted argv + may read stdin; check temp-file handling, no shell-out, arg injection) + fix wave + push.

## Self-Review
Covers the spec's drop-in-CLI goal (common-flag subset; exhaustive parity is iterative). Shares the registry with the C ABI (one settings path). Deferred: full ~100-flag parity, manpage exactness, `--read-args-from-stdin`, image CLI (M5). Security: argv/stdin are the untrusted inputs — no shell execution, temp files via `tempfile`, paths validated.
