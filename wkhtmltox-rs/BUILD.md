# Building wkhtmltox-rs from source

## Prerequisites

| Requirement | Notes |
|:------------|:------|
| **Rust stable** (MSRV 1.74) | Pin managed by `rust-toolchain.toml`; install via [rustup](https://rustup.rs) |
| **qpdf dev headers** | Required by `wkhtmltox-pdf-sys/build.rs` via pkg-config |
| **C++17 compiler** | `g++ >= 7` or `clang++ >= 5` (called by the `cc` crate during build) |

Install qpdf:

```sh
# Debian / Ubuntu
sudo apt install libqpdf-dev qpdf

# macOS (Homebrew)
brew install qpdf
```

## Build

```sh
cd wkhtmltox-rs
cargo build --release
```

Produced artifacts (under `wkhtmltox-rs/target/release/`):

| File | Description |
|:-----|:------------|
| `wkhtmltopdf` | PDF CLI tool |
| `wkhtmltoimage` | Image CLI tool |
| `libwkhtmltox.so` / `libwkhtmltox.dylib` | C ABI shared library |
| `libwkhtmltox.a` | C ABI static library |

C headers live at `crates/wkhtmltox-capi/include/` (`pdf.h`, `image.h`).

## Rendering engine (chrome-headless-shell)

Both CLI tools and the C library require a `chrome-headless-shell` binary to render HTML. The discovery order is:

1. **`WKHTMLTOX_CHROME` environment variable** — if set, that path is used exactly. If the path does not exist the tools exit with an error; the platform-default search is **not** attempted when this variable is set.
2. **Bundled sibling** — a `chrome-headless-shell` binary (or `chrome-headless-shell/chrome-headless-shell` subdirectory layout) placed next to the executable is picked up automatically.
3. **System Chrome candidates** — common install locations on Linux and macOS (e.g. `/usr/bin/google-chrome`, `/Applications/Google Chrome.app/…`).

A packaged tarball (see [Packaging](#packaging)) bundles `chrome-headless-shell` under `bin/` and is used automatically via rule 2 — no system Chrome is needed.

For a source build without a system Chrome:

```sh
export WKHTMLTOX_CHROME=/path/to/chrome-headless-shell
./target/release/wkhtmltopdf input.html output.pdf
```

## Packaging

Run from the **repository root** (not from `wkhtmltox-rs/`):

```sh
# Full package — downloads and bundles chrome-headless-shell (~150–200 MB)
bash scripts/package.sh

# Skip Chrome download (fast; useful for CI steps that supply Chrome separately)
bash scripts/package.sh --no-chrome

# Pin a specific Chrome for Testing version
CHROME_VERSION=131.0.6778.204 bash scripts/package.sh
```

Output: `dist/wkhtmltox-rs-<version>-<platform>.tar.gz`

Artifact layout inside the tarball:

```
bin/wkhtmltopdf
bin/wkhtmltoimage
bin/chrome-headless-shell/chrome-headless-shell   # full package only
lib/libwkhtmltox.{so,dylib}
lib/libwkhtmltox.a
include/pdf.h
include/image.h
LICENSE
README.txt
```

Supported platforms (v1): `linux-x86_64`, `macos-arm64`, `macos-x86_64`. Windows is post-v1.

> **Size note:** The bundled `chrome-headless-shell` adds approximately 150–200 MB to the tarball.

## Running CI checks locally

These mirror `.github/workflows/ci.yml`. Run from `wkhtmltox-rs/` unless noted.

### Formatting

```sh
cargo fmt --all --check
```

### Lints

```sh
cargo clippy --workspace --lib -- -D warnings
```

### Unit and integration tests

```sh
cargo test --workspace
```

### License and dependency policy (cargo-deny)

```sh
cargo deny check          # install: cargo install cargo-deny
```

### Advisory scan (cargo-audit)

```sh
cargo audit               # install: cargo install cargo-audit
```

### Static security audit

Run from the **repository root**:

```sh
bash scripts/security-audit.sh --full
```

Exit 0 = clean; exit 2 = HIGH-severity finding(s). The report is written to `.superpowers/security/`.

### Compat gate (oracle comparison)

Requires the wkhtmltopdf 0.12.6 oracle binary, a Chrome / `chrome-headless-shell`, and Python dependencies:

```sh
pip install pymupdf numpy scikit-image
```

```sh
# Run from wkhtmltox-rs/
WKHTMLTOX_ORACLE=/path/to/wkhtmltopdf-0.12.6 \
  python3 tests/compat/compare.py --gate
```

`--gate` checks aggregate fidelity metrics against the thresholds in `tests/compat/thresholds.json` and exits 1 if any metric falls below its floor.

## Gated Chrome end-to-end tests

These tests are skipped in a normal `cargo test` run (marked `#[ignore]`) and require a real Chrome or `chrome-headless-shell`:

```sh
cargo test -- --ignored --test-threads=1
```
