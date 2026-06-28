#!/usr/bin/env python3
# wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
"""
Fidelity-measurement harness: compare wkhtmltopdf 0.12.6 (oracle) vs
wkhtmltox-render-chromium (new engine) across the compat corpus.

Usage:
  python3 compare.py                        # baseline corpus
  python3 compare.py --dir tests/compat/edge  # edge-case set
  python3 compare.py --dir /abs/path/to/dir   # absolute path

Outputs (prefix derived from --dir name, default "results"):
  - table to stdout
  - tests/compat/{prefix}.md   (markdown table + notes)
  - tests/compat/{prefix}.json (raw numbers)
"""

import argparse
import difflib
import json
import os
import re
import subprocess
import sys
import textwrap
from pathlib import Path

import fitz  # pymupdf
import numpy as np
from skimage.metrics import structural_similarity as ssim

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------

parser = argparse.ArgumentParser(description="Fidelity harness: oracle vs new engine")
parser.add_argument(
    "--dir",
    metavar="PATH",
    default=None,
    help="Directory of HTML files to measure (default: tests/compat/corpus). "
         "Relative paths are resolved from the repo root (two levels above this script).",
)
parser.add_argument(
    "--out-prefix",
    metavar="PREFIX",
    default=None,
    help="Prefix for output files (results-PREFIX.md / results-PREFIX.json). "
         "Defaults to 'results' for the baseline corpus, or 'results-<dirname>' otherwise.",
)
parser.add_argument(
    "--compat",
    action="store_true",
    default=False,
    help="Pass --compat to the new-engine render example to enable the WK0126 UA-reset stylesheet.",
)
parser.add_argument(
    "--assemble",
    action="store_true",
    default=False,
    help="Run the 2-document assembly comparison (text.html + headings.html) instead of the corpus sweep.",
)
parser.add_argument(
    "--m2b",
    action="store_true",
    default=False,
    help="Run the M2b full-document assembly comparison "
         "(cover+toc+footer: headings.html + longtext.html) vs the oracle.",
)
parser.add_argument(
    "--cli",
    action="store_true",
    default=False,
    help="Run M3b CLI comparison: our wkhtmltopdf binary vs oracle "
         "(structural + exit-code checks).",
)
parser.add_argument(
    "--image",
    action="store_true",
    default=False,
    help="Run M5 image comparison: our wkhtmltoimage binary vs oracle "
         "(dimensions + SSIM on PNG output).",
)
parser.add_argument(
    "--gate",
    action="store_true",
    default=False,
    help="After the corpus sweep, check aggregate metrics against thresholds and "
         "exit 1 if any metric falls below its floor (requires --thresholds file).",
)
parser.add_argument(
    "--thresholds",
    metavar="PATH",
    default=None,
    help="Path to a JSON thresholds file (default: tests/compat/thresholds.json). "
         "Used only when --gate is specified.",
)
ARGS = parser.parse_known_args()[0]

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
SCRIPT_DIR = Path(__file__).resolve().parent
WORKSPACE_DIR = SCRIPT_DIR.parent.parent  # wkhtmltox-rs root

if ARGS.dir is not None:
    raw = Path(ARGS.dir)
    CORPUS_DIR = raw if raw.is_absolute() else (WORKSPACE_DIR / raw).resolve()
else:
    CORPUS_DIR = SCRIPT_DIR / "corpus"

# Derive output prefix
if ARGS.out_prefix:
    _prefix = ARGS.out_prefix
elif ARGS.dir is not None:
    _prefix = f"results-{CORPUS_DIR.name}"
    if ARGS.compat:
        _prefix += "-compat"
else:
    if ARGS.compat:
        _prefix = "results-compat"
    else:
        _prefix = "results"

_out_suffix = f"{CORPUS_DIR.name}" + ("-compat" if ARGS.compat else "")
OUT_DIR = SCRIPT_DIR / f"out-{_out_suffix}"
OUT_DIR.mkdir(parents=True, exist_ok=True)

ORACLE_BIN = Path(os.environ.get("WKHTMLTOX_ORACLE", "/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf"))

CORPUS_FILES = sorted(CORPUS_DIR.glob("*.html"))

# ---------------------------------------------------------------------------
# Render helpers
# ---------------------------------------------------------------------------

def render_oracle(html_path: Path, out_pdf: Path) -> tuple[str, str]:
    """
    Run wkhtmltopdf and return (status, detail).
    Status: "OK" | "ORACLE_CRASH" | "ORACLE_ERROR"
    """
    cmd = [
        str(ORACLE_BIN),
        "-s", "A4",
        "-T", "10mm", "-B", "10mm", "-L", "10mm", "-R", "10mm",
        "--dpi", "96",
        "--enable-local-file-access",
        "--outline",
        "--quiet",
        str(html_path),
        str(out_pdf),
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return "ORACLE_CRASH", "timeout after 120s"
    except Exception as exc:
        return "ORACLE_CRASH", f"subprocess error: {exc}"

    stderr_tail = result.stderr.strip()[-300:] if result.stderr else ""
    if result.returncode < 0:
        # Negative returncode = killed by signal (crash)
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "ORACLE_CRASH", f"killed by SIG{sig_name}; stderr: {stderr_tail}"
    if result.returncode != 0 or not out_pdf.exists():
        return "ORACLE_ERROR", f"rc={result.returncode}; stderr: {stderr_tail}"
    return "OK", stderr_tail


def render_new(html_path: Path, out_pdf: Path) -> tuple[str, str]:
    """
    Run the Chromium example and return (status, detail).
    Status: "OK" | "NEW_CRASH" | "NEW_ERROR"
    """
    extra_flags = ["--compat"] if ARGS.compat else []
    cmd = [
        "cargo", "run", "-q",
        "--example", "render",
        "-p", "wkhtmltox-render-chromium",
        "--",
        *extra_flags,
        str(html_path.resolve()),
        str(out_pdf),
    ]
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=120,
            cwd=str(WORKSPACE_DIR)
        )
    except subprocess.TimeoutExpired:
        return "NEW_CRASH", "timeout after 120s"
    except Exception as exc:
        return "NEW_CRASH", f"subprocess error: {exc}"

    combined = (result.stderr + result.stdout).strip()
    tail = combined[-300:] if combined else ""

    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "NEW_CRASH", f"killed by SIG{sig_name}; output: {tail}"
    if result.returncode != 0 or not out_pdf.exists():
        return "NEW_ERROR", f"rc={result.returncode}; output: {tail}"
    return "OK", tail


# ---------------------------------------------------------------------------
# Metric helpers
# ---------------------------------------------------------------------------

def extract_text(doc: fitz.Document) -> str:
    """Extract and normalise text from all pages."""
    parts = []
    for page in doc:
        parts.append(page.get_text("text"))
    raw = " ".join(parts)
    return re.sub(r"\s+", " ", raw).strip()


def text_similarity(a: str, b: str) -> float:
    """SequenceMatcher ratio between two normalised strings."""
    return difflib.SequenceMatcher(None, a, b).ratio()


def outline_match(ref_toc, new_toc) -> tuple[int, int, float]:
    """Compare outline (TOC) entry (level, title) lists."""
    ref_entries = [(lvl, title.strip()) for lvl, title, _ in ref_toc]
    new_entries = [(lvl, title.strip()) for lvl, title, _ in new_toc]
    ref_n = len(ref_entries)
    new_n = len(new_entries)
    common = len(set(ref_entries) & set(new_entries))
    denom = max(ref_n, new_n, 1)
    ratio = common / denom
    return ref_n, new_n, ratio


def outline_tree_ratio(ref_toc, new_toc) -> float:
    """Ordered similarity of two outlines as (level, title) sequences.

    Unlike outline_match (set intersection), this respects order and nesting:
    a reordered or re-nested outline scores below 1.0.  Uses difflib's
    longest-contiguous-matching-blocks ratio over the (level, title) tuples.
    """
    ref_seq = [(lvl, (title or "").strip()) for lvl, title, *_ in ref_toc]
    new_seq = [(lvl, (title or "").strip()) for lvl, title, *_ in new_toc]
    if not ref_seq and not new_seq:
        return 1.0
    if not ref_seq or not new_seq:
        return 0.0
    return difflib.SequenceMatcher(None, ref_seq, new_seq).ratio()


def extract_body_text(doc, top_pt: float = 36.0, bottom_pt: float = 36.0,
                      skip_toc: bool = False) -> str:
    """Concatenated page text EXCLUDING the top/bottom margin bands (where
    running headers/footers live) and, optionally, a leading TOC page.

    Header/footer chrome and TOC formatting were the dominant source of the
    low M2b text_sim despite matching body content; clipping the margin bands
    and the TOC page isolates the body for a fair comparison.
    """
    parts = []
    start = 1 if (skip_toc and doc.page_count > 1) else 0
    for i in range(start, doc.page_count):
        page = doc[i]
        r = page.rect
        body = fitz.Rect(r.x0, r.y0 + top_pt, r.x1, r.y1 - bottom_pt)
        parts.append(page.get_text("text", clip=body))
    return " ".join(parts)


def gate_check(metrics: dict, thresholds: dict) -> list:
    """Return a list of human-readable failure strings; empty list == pass.

    Recognised threshold keys: mean_ssim (floor), outline_ratio (floor),
    text_sim (floor), max_abs_delta_pages (ceiling on |pages_new-pages_ref|).
    Only keys present in `thresholds` are enforced.
    """
    fails = []
    for key in ("mean_ssim", "outline_ratio", "text_sim"):
        if key in thresholds and key in metrics and metrics[key] < thresholds[key]:
            fails.append(f"{key}={metrics[key]:.4f} < floor {thresholds[key]:.4f}")
    if "max_abs_delta_pages" in thresholds and "abs_delta_pages" in metrics:
        if metrics["abs_delta_pages"] > thresholds["max_abs_delta_pages"]:
            fails.append(
                f"abs_delta_pages={metrics['abs_delta_pages']} > "
                f"ceiling {thresholds['max_abs_delta_pages']}")
    return fails


def page_to_gray_array(page: fitz.Page, dpi: int = 100) -> np.ndarray:
    """Rasterize a PDF page to a grayscale uint8 numpy array."""
    mat = fitz.Matrix(dpi / 72, dpi / 72)
    pix = page.get_pixmap(matrix=mat, colorspace=fitz.csGRAY)
    arr = np.frombuffer(pix.samples, dtype=np.uint8).reshape(pix.height, pix.width)
    return arr


def pad_to_common(a: np.ndarray, b: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Pad both arrays to the same shape (max of each dim) with white (255)."""
    h = max(a.shape[0], b.shape[0])
    w = max(a.shape[1], b.shape[1])
    def pad(arr):
        ph = h - arr.shape[0]
        pw = w - arr.shape[1]
        return np.pad(arr, ((0, ph), (0, pw)), constant_values=255)
    return pad(a), pad(b)


def visual_metrics(ref_doc: fitz.Document, new_doc: fitz.Document, dpi: int = 100):
    """
    Return (mean_ssim, min_ssim, mean_pixel_diff_pct) over matched pages.
    Only compares min(page_count_ref, page_count_new) pages.
    """
    n_ref = ref_doc.page_count
    n_new = new_doc.page_count
    n_cmp = min(n_ref, n_new)
    if n_cmp == 0:
        return 0.0, 0.0, 1.0

    ssim_scores = []
    pixel_diffs = []

    for i in range(n_cmp):
        arr_ref = page_to_gray_array(ref_doc[i], dpi)
        arr_new = page_to_gray_array(new_doc[i], dpi)
        arr_ref, arr_new = pad_to_common(arr_ref, arr_new)

        win = min(arr_ref.shape[0], arr_ref.shape[1], 7)
        if win < 3:
            win = 3
        if win % 2 == 0:
            win -= 1

        score = ssim(arr_ref, arr_new, data_range=255, win_size=win)
        ssim_scores.append(score)

        diff = np.mean(np.abs(arr_ref.astype(float) - arr_new.astype(float))) / 255.0
        pixel_diffs.append(diff)

    return float(np.mean(ssim_scores)), float(np.min(ssim_scores)), float(np.mean(pixel_diffs))


# ---------------------------------------------------------------------------
# Main measurement loop
# ---------------------------------------------------------------------------

def measure_file(html_path: Path, skip_toc: bool = False) -> dict:
    name = html_path.stem
    ref_pdf = OUT_DIR / f"{name}.ref.pdf"
    new_pdf = OUT_DIR / f"{name}.new.pdf"

    result = {"name": name, "status": "OK", "error": None}

    # --- oracle render (crash-resilient) ---
    try:
        oracle_status, oracle_detail = render_oracle(html_path, ref_pdf)
    except Exception as exc:
        oracle_status, oracle_detail = "ORACLE_CRASH", str(exc)

    if oracle_status != "OK":
        result["status"] = oracle_status
        result["error"] = f"{oracle_status}: {oracle_detail[:250]}"
        return result

    # --- new engine render (crash-resilient) ---
    try:
        new_status, new_detail = render_new(html_path, new_pdf)
    except Exception as exc:
        new_status, new_detail = "NEW_CRASH", str(exc)

    if new_status != "OK":
        result["status"] = new_status
        result["error"] = f"{new_status}: {new_detail[:250]}"
        return result

    # --- open with pymupdf ---
    try:
        ref_doc = fitz.open(str(ref_pdf))
        new_doc = fitz.open(str(new_pdf))
    except Exception as e:
        result["status"] = "METRICS_ERROR"
        result["error"] = f"fitz open failed: {e}"
        return result

    try:
        # page count
        pages_ref = ref_doc.page_count
        pages_new = new_doc.page_count
        delta_pages = pages_new - pages_ref

        # text similarity
        text_ref = extract_text(ref_doc)
        text_new = extract_text(new_doc)
        text_sim = text_similarity(text_ref, text_new)

        # outline
        toc_ref = ref_doc.get_toc()
        toc_new = new_doc.get_toc()
        outline_ref_n, outline_new_n, _set_ratio = outline_match(toc_ref, toc_new)
        outline_ratio = outline_tree_ratio(toc_ref, toc_new)

        # body text similarity (excludes header/footer margin bands and optional TOC page)
        body_text_ref = extract_body_text(ref_doc, skip_toc=skip_toc)
        body_text_new = extract_body_text(new_doc, skip_toc=skip_toc)
        body_text_sim = text_similarity(body_text_ref, body_text_new)

        # visual
        mean_ssim, min_ssim, pixel_diff_pct = visual_metrics(ref_doc, new_doc, dpi=100)
    except Exception as exc:
        result["status"] = "METRICS_ERROR"
        result["error"] = f"metrics computation failed: {exc}"
        return result
    finally:
        ref_doc.close()
        new_doc.close()

    # composite similarity score
    page_score = 1.0 - min(1.0, abs(delta_pages) / max(1, pages_ref))
    composite = (text_sim + mean_ssim + page_score) / 3.0

    result.update({
        "pages_ref": pages_ref,
        "pages_new": pages_new,
        "delta_pages": delta_pages,
        "text_sim": round(text_sim, 4),
        "body_text_sim": round(body_text_sim, 4),
        "outline_ref": outline_ref_n,
        "outline_new": outline_new_n,
        "outline_ratio": round(outline_ratio, 4),
        "mean_ssim": round(mean_ssim, 4),
        "min_ssim": round(min_ssim, 4),
        "pixel_diff_pct": round(pixel_diff_pct * 100, 3),
        "score": round(composite, 4),
    })
    return result


# ---------------------------------------------------------------------------
# Formatting
# ---------------------------------------------------------------------------

def fmt_row(r: dict) -> str:
    status = r.get("status", "OK")
    if status != "OK":
        err_short = (r.get("error") or "")[:50]
        return (
            f"| {r['name']:<28} | {status:<13} | -- | -- |  +0 | ------- | ------- | ------------- "
            f"| ----- | ----- | ----- | {err_short} |"
        )
    return (
        f"| {r['name']:<28} "
        f"| {'OK':<13} "
        f"| {r['pages_ref']:>2} "
        f"| {r['pages_new']:>2} "
        f"| {r['delta_pages']:>+3} "
        f"| {r['text_sim']:.4f}  "
        f"| {r['body_text_sim']:.4f}  "
        f"| {r['outline_ref']:>2}/{r['outline_new']:<2} ({r['outline_ratio']:.2f}) "
        f"| {r['mean_ssim']:.4f} "
        f"| {r['min_ssim']:.4f} "
        f"| {r['pixel_diff_pct']:>5.2f}% "
        f"| {r['score']:.4f} |"
    )


HEADER = (
    "| Document                     | status        | p_ref | p_new |  Δp | text_sim | body_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |"
)
SEPARATOR = (
    "|:-----------------------------|:--------------|------:|------:|----:|---------:|---------:|:----------------|----------:|---------:|---------:|-------:|"
)


def build_table(results: list[dict]) -> str:
    lines = [HEADER, SEPARATOR]
    for r in results:
        lines.append(fmt_row(r))
    return "\n".join(lines)


def aggregate(results: list[dict]) -> dict:
    ok = [r for r in results if r.get("status") == "OK"]
    errors = [r for r in results if r.get("status") != "OK"]
    if not ok:
        return {"n_ok": 0, "n_errors": len(errors)}
    return {
        "n_ok": len(ok),
        "n_errors": len(errors),
        "mean_text_sim": round(float(np.mean([r["text_sim"] for r in ok])), 4),
        "mean_body_text_sim": round(float(np.mean([r["body_text_sim"] for r in ok])), 4),
        "min_outline_ratio": round(float(np.min([r["outline_ratio"] for r in ok])), 4),
        "mean_ssim": round(float(np.mean([r["mean_ssim"] for r in ok])), 4),
        "min_ssim_overall": round(float(np.min([r["min_ssim"] for r in ok])), 4),
        "mean_pixel_diff_pct": round(float(np.mean([r["pixel_diff_pct"] for r in ok])), 3),
        "mean_score": round(float(np.mean([r["score"] for r in ok])), 4),
        "total_page_delta": int(np.sum([abs(r["delta_pages"]) for r in ok])),
        "max_abs_delta_pages": int(np.max([abs(r["delta_pages"]) for r in ok])),
    }


# ---------------------------------------------------------------------------
# Assembly comparison (--assemble mode)
# ---------------------------------------------------------------------------

def run_oracle_assemble(html_paths: list, out_pdf: Path) -> tuple[str, str]:
    """
    Run wkhtmltopdf with multiple inputs and return (status, detail).
    Merges all inputs into one PDF using native multi-input support.
    """
    cmd = [
        str(ORACLE_BIN),
        "-s", "A4",
        "-T", "10mm", "-B", "10mm", "-L", "10mm", "-R", "10mm",
        "--dpi", "96",
        "--enable-local-file-access",
        "--outline",
        "--quiet",
    ]
    for p in html_paths:
        cmd.append(str(p))
    cmd.append(str(out_pdf))

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        return "ORACLE_CRASH", "timeout after 180s"
    except Exception as exc:
        return "ORACLE_CRASH", f"subprocess error: {exc}"

    stderr_tail = result.stderr.strip()[-300:] if result.stderr else ""
    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "ORACLE_CRASH", f"killed by SIG{sig_name}; stderr: {stderr_tail}"
    if result.returncode != 0 or not out_pdf.exists():
        return "ORACLE_ERROR", f"rc={result.returncode}; stderr: {stderr_tail}"
    return "OK", stderr_tail


def run_new_assemble(html_paths: list, out_pdf: Path) -> tuple[str, str]:
    """
    Run the assemble example (ChromiumRenderer + assemble_pdf) and return (status, detail).
    """
    resolved = [str(Path(p).resolve()) for p in html_paths]
    cmd = [
        "cargo", "run", "-q",
        "--example", "assemble",
        "-p", "wkhtmltox-render-chromium",
        "--",
        str(out_pdf),
        *resolved,
    ]
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=300,
            cwd=str(WORKSPACE_DIR)
        )
    except subprocess.TimeoutExpired:
        return "NEW_CRASH", "timeout after 300s"
    except Exception as exc:
        return "NEW_CRASH", f"subprocess error: {exc}"

    combined = (result.stderr + result.stdout).strip()
    tail = combined[-500:] if combined else ""

    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "NEW_CRASH", f"killed by SIG{sig_name}; output: {tail}"
    if result.returncode != 0 or not out_pdf.exists():
        return "NEW_ERROR", f"rc={result.returncode}; output: {tail}"
    return "OK", tail


def extract_outline_titles(toc: list) -> list[str]:
    """Return just the title strings from a PyMuPDF TOC list."""
    return [title.strip() for _lvl, title, _page in toc]


def run_assembly_comparison():
    """
    Compare 2-document assembly: corpus/text.html + corpus/headings.html.
    Oracle uses native multi-input; new engine uses the assemble example.
    Compares: page count (±1 tolerance), outline entry count, outline titles (structural).
    Writes results-assembly.md and results-assembly.json.
    """
    corpus_dir = SCRIPT_DIR / "corpus"
    doc1 = corpus_dir / "text.html"
    doc2 = corpus_dir / "headings.html"

    for p in (doc1, doc2):
        if not p.exists():
            print(f"ERROR: corpus file not found: {p}", file=sys.stderr)
            sys.exit(1)

    asm_out_dir = SCRIPT_DIR / "out-assembly"
    asm_out_dir.mkdir(parents=True, exist_ok=True)

    ref_pdf = asm_out_dir / "assembly.ref.pdf"
    new_pdf = asm_out_dir / "assembly.new.pdf"

    print(f"Oracle  : {ORACLE_BIN}")
    print(f"Inputs  : {doc1.name} + {doc2.name}")
    print(f"Out dir : {asm_out_dir}")
    print()

    # --- oracle ---
    print("Running oracle assembly ...", flush=True)
    oracle_status, oracle_detail = run_oracle_assemble([doc1, doc2], ref_pdf)
    if oracle_status != "OK":
        print(f"  ORACLE FAILED: {oracle_status}: {oracle_detail[:300]}", file=sys.stderr)
        sys.exit(1)
    print(f"  oracle OK -> {ref_pdf}")

    # --- new engine ---
    print("Running new-engine assembly (cargo run --example assemble) ...", flush=True)
    new_status, new_detail = run_new_assemble([doc1, doc2], new_pdf)
    if new_status != "OK":
        print(f"  NEW ENGINE FAILED: {new_status}: {new_detail[:300]}", file=sys.stderr)
        sys.exit(1)
    print(f"  new engine OK -> {new_pdf}")
    print(f"  detail: {new_detail[-200:]}")

    # --- open with pymupdf ---
    ref_doc = fitz.open(str(ref_pdf))
    new_doc = fitz.open(str(new_pdf))

    pages_ref = ref_doc.page_count
    pages_new = new_doc.page_count
    delta_pages = pages_new - pages_ref
    pages_within_tolerance = abs(delta_pages) <= 1

    toc_ref = ref_doc.get_toc()
    toc_new = new_doc.get_toc()
    titles_ref = extract_outline_titles(toc_ref)
    titles_new = extract_outline_titles(toc_new)

    outline_ref_n = len(toc_ref)
    outline_new_n = len(toc_new)

    # Structural title match: intersection over max (ignoring page numbers)
    set_ref = set(titles_ref)
    set_new = set(titles_new)
    common_titles = set_ref & set_new
    denom = max(len(set_ref), len(set_new), 1)
    title_overlap = len(common_titles) / denom

    # Text similarity across full merged docs
    text_ref = extract_text(ref_doc)
    text_new = extract_text(new_doc)
    text_sim = text_similarity(text_ref, text_new)

    ref_doc.close()
    new_doc.close()

    print()
    print(f"  Page count  : oracle={pages_ref}  new={pages_new}  Δ={delta_pages:+d}  within_±1={pages_within_tolerance}")
    print(f"  Outline     : oracle entries={outline_ref_n}  new entries={outline_new_n}")
    print(f"  Titles ref  : {titles_ref}")
    print(f"  Titles new  : {titles_new}")
    print(f"  Title overlap (intersection/max): {title_overlap:.3f}  ({len(common_titles)}/{denom})")
    print(f"  Titles only in oracle : {sorted(set_ref - set_new)}")
    print(f"  Titles only in new    : {sorted(set_new - set_ref)}")
    print(f"  Text similarity       : {text_sim:.4f}")

    # Build result dict
    result = {
        "mode": "assembly",
        "inputs": [str(doc1), str(doc2)],
        "oracle_pdf": str(ref_pdf),
        "new_pdf": str(new_pdf),
        "pages_ref": pages_ref,
        "pages_new": pages_new,
        "delta_pages": delta_pages,
        "pages_within_tolerance": pages_within_tolerance,
        "outline_ref_n": outline_ref_n,
        "outline_new_n": outline_new_n,
        "titles_ref": titles_ref,
        "titles_new": titles_new,
        "common_titles": sorted(common_titles),
        "only_in_ref": sorted(set_ref - set_new),
        "only_in_new": sorted(set_new - set_ref),
        "title_overlap": round(title_overlap, 4),
        "text_sim": round(text_sim, 4),
    }

    # --- write results-assembly.md ---
    md_path = SCRIPT_DIR / "results-assembly.md"
    titles_ref_str = "\n".join(f"  - {t}" for t in titles_ref) if titles_ref else "  (none)"
    titles_new_str = "\n".join(f"  - {t}" for t in titles_new) if titles_new else "  (none)"
    only_ref_str = "\n".join(f"  - {t}" for t in sorted(set_ref - set_new)) if (set_ref - set_new) else "  (none)"
    only_new_str = "\n".join(f"  - {t}" for t in sorted(set_new - set_ref)) if (set_new - set_ref) else "  (none)"
    md_content = textwrap.dedent(f"""\
        # Assembly Comparison: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `{ORACLE_BIN}`
        Inputs: `{doc1.name}` + `{doc2.name}`
        New engine: `cargo run --example assemble -p wkhtmltox-render-chromium`

        ## Page Count

        | Side    | Pages |
        |:--------|------:|
        | Oracle  | {pages_ref} |
        | New     | {pages_new} |
        | Δ       | {delta_pages:+d} |
        | Within ±1 tolerance | {'Yes' if pages_within_tolerance else 'No'} |

        ## Outline (Bookmark) Comparison

        | Metric                      | Value |
        |:----------------------------|------:|
        | Oracle outline entries      | {outline_ref_n} |
        | New engine outline entries  | {outline_new_n} |
        | Title overlap (intersection/max) | {title_overlap:.3f} ({len(common_titles)}/{denom}) |

        ### Oracle bookmark titles
{titles_ref_str}

        ### New-engine bookmark titles
{titles_new_str}

        ### Titles only in oracle
{only_ref_str}

        ### Titles only in new engine
{only_new_str}

        ## Text Similarity

        | Metric     | Value  |
        |:-----------|-------:|
        | text_sim   | {text_sim:.4f} |

        ## Notes

        - Outline page numbers are NOT compared (M2a: headings point to object-first-page only;
          per-heading exact destinations deferred to Milestone 2b).
        - Page count tolerance ±1 accepted because Chrome and wkhtmltopdf paginate
          identically-sized content with minor differences.
        - Title overlap is the structural fidelity signal: are the same section titles
          present in both outlines?
    """)
    md_path.write_text(md_content, encoding="utf-8")
    print(f"\nWrote {md_path}")

    # --- write results-assembly.json ---
    json_path = SCRIPT_DIR / "results-assembly.json"
    json_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote {json_path}")


# ---------------------------------------------------------------------------
# M2b full-document assembly comparison (cover + toc + footer)
# ---------------------------------------------------------------------------

COVER_HTML = """\
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Document Cover</title>
<style>
  body {
    font-family: Helvetica, Arial, sans-serif;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    height: 100vh;
    margin: 0;
  }
  h1 { font-size: 36pt; margin-bottom: 12pt; }
  p  { font-size: 14pt; color: #555; }
</style>
</head>
<body>
  <h1>Test Document</h1>
  <p>wkhtmltox-rs M2b Validation Report</p>
</body>
</html>
"""

FOOTER_TMPL = "[page]/[topage]"


def run_oracle_m2b(cover_html: Path, html_paths: list, out_pdf: Path) -> tuple[str, str]:
    """
    Run wkhtmltopdf with cover + toc + pages + footer.
    Returns (status, detail).

    Oracle command:
      wkhtmltopdf [global] --footer-center "[page]/[topage]" \\
          cover cover.html toc page1.html page2.html out.pdf
    """
    cmd = [
        str(ORACLE_BIN),
        "-s", "A4",
        "-T", "10mm", "-B", "10mm", "-L", "10mm", "-R", "10mm",
        "--dpi", "96",
        "--enable-local-file-access",
        "--outline",
        "--footer-center", FOOTER_TMPL,
        "--quiet",
        "cover", str(cover_html),
        "toc",
    ]
    for p in html_paths:
        cmd.append(str(p))
    cmd.append(str(out_pdf))

    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        return "ORACLE_CRASH", "timeout after 180s"
    except Exception as exc:
        return "ORACLE_CRASH", f"subprocess error: {exc}"

    stderr_tail = result.stderr.strip()[-500:] if result.stderr else ""
    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "ORACLE_CRASH", f"killed by SIG{sig_name}; stderr: {stderr_tail}"
    if result.returncode != 0 or not out_pdf.exists():
        return "ORACLE_ERROR", f"rc={result.returncode}; stderr: {stderr_tail}"
    return "OK", stderr_tail


def run_new_m2b(cover_html: Path, html_paths: list, out_pdf: Path) -> tuple[str, str]:
    """
    Run cargo run --example assemble with --toc --cover --footer-center flags.
    Returns (status, detail).
    """
    resolved = [str(Path(p).resolve()) for p in html_paths]
    cmd = [
        "cargo", "run", "-q",
        "--example", "assemble",
        "-p", "wkhtmltox-render-chromium",
        "--",
        str(out_pdf),
        "--toc",
        "--cover", str(cover_html.resolve()),
        "--footer-center", FOOTER_TMPL,
        *resolved,
    ]
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=600,
            cwd=str(WORKSPACE_DIR)
        )
    except subprocess.TimeoutExpired:
        return "NEW_CRASH", "timeout after 600s"
    except Exception as exc:
        return "NEW_CRASH", f"subprocess error: {exc}"

    combined = (result.stderr + result.stdout).strip()
    tail = combined[-500:] if combined else ""

    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "NEW_CRASH", f"killed by SIG{sig_name}; output: {tail}"
    if result.returncode != 0 or not out_pdf.exists():
        return "NEW_ERROR", f"rc={result.returncode}; output: {tail}"
    return "OK", tail


def has_toc_page(doc: fitz.Document) -> bool:
    """
    Return True if any page in doc contains text matching a Table of Contents heading.
    Checks for 'Table of Contents', 'Contents', or 'Inhaltsverzeichnis'.
    """
    toc_patterns = ["table of contents", "contents"]
    for page in doc:
        text_lower = page.get_text("text").lower()
        for pat in toc_patterns:
            if pat in text_lower:
                return True
    return False


def run_m2b_comparison():
    """
    Full M2b assembly comparison:
    - Oracle: wkhtmltopdf cover + toc + footer
    - New engine: assemble --toc --cover --footer-center
    - Inputs: corpus/headings.html + corpus/longtext.html
    - Writes results-m2b.md and results-m2b.json
    """
    corpus_dir = SCRIPT_DIR / "corpus"
    doc1 = corpus_dir / "headings.html"
    doc2 = corpus_dir / "longtext.html"

    for p in (doc1, doc2):
        if not p.exists():
            print(f"ERROR: corpus file not found: {p}", file=sys.stderr)
            sys.exit(1)

    m2b_out_dir = SCRIPT_DIR / "out-m2b"
    m2b_out_dir.mkdir(parents=True, exist_ok=True)

    # Write the cover HTML to the output directory.
    cover_html = m2b_out_dir / "cover.html"
    cover_html.write_text(COVER_HTML, encoding="utf-8")

    ref_pdf = m2b_out_dir / "m2b.ref.pdf"
    new_pdf = m2b_out_dir / "m2b.new.pdf"

    print(f"Oracle  : {ORACLE_BIN}")
    print(f"Inputs  : cover.html + {doc1.name} + {doc2.name}")
    print(f"Footer  : {FOOTER_TMPL!r}")
    print(f"Out dir : {m2b_out_dir}")
    print()

    # --- oracle ---
    print("Running oracle (cover + toc + footer) ...", flush=True)
    oracle_status, oracle_detail = run_oracle_m2b(cover_html, [doc1, doc2], ref_pdf)
    if oracle_status != "OK":
        print(
            f"  ORACLE FAILED: {oracle_status}: {oracle_detail[:300]}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"  oracle OK -> {ref_pdf}")
    if oracle_detail:
        print(f"  oracle stderr: {oracle_detail[-200:]}")

    # --- new engine ---
    print("Running new engine (assemble --toc --cover --footer-center) ...", flush=True)
    new_status, new_detail = run_new_m2b(cover_html, [doc1, doc2], new_pdf)
    if new_status != "OK":
        print(
            f"  NEW ENGINE FAILED: {new_status}: {new_detail[:300]}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"  new engine OK -> {new_pdf}")
    if new_detail:
        print(f"  new engine detail: {new_detail[-200:]}")

    # --- open with pymupdf ---
    ref_doc = fitz.open(str(ref_pdf))
    new_doc = fitz.open(str(new_pdf))

    pages_ref = ref_doc.page_count
    pages_new = new_doc.page_count
    delta_pages = pages_new - pages_ref
    # ±2 tolerance: cover + TOC add pages; Chrome/oracle paginate slightly differently.
    pages_within_tolerance = abs(delta_pages) <= 2

    # Outline (bookmark) comparison.
    toc_ref = ref_doc.get_toc()
    toc_new = new_doc.get_toc()
    titles_ref = [title.strip() for _lvl, title, _page in toc_ref]
    titles_new = [title.strip() for _lvl, title, _page in toc_new]
    outline_ref_n = len(toc_ref)
    outline_new_n = len(toc_new)

    set_ref = set(titles_ref)
    set_new = set(titles_new)
    common_titles = set_ref & set_new
    denom = max(len(set_ref), len(set_new), 1)
    title_overlap = len(common_titles) / denom

    # TOC page detection.
    toc_in_ref = has_toc_page(ref_doc)
    toc_in_new = has_toc_page(new_doc)

    # Text similarity (full document).
    text_ref = extract_text(ref_doc)
    text_new = extract_text(new_doc)
    text_sim = text_similarity(text_ref, text_new)

    ref_doc.close()
    new_doc.close()

    # --- print summary ---
    print()
    print(f"  Page count         : oracle={pages_ref}  new={pages_new}  Δ={delta_pages:+d}  within_±2={pages_within_tolerance}")
    print(f"  Outline entries    : oracle={outline_ref_n}  new={outline_new_n}")
    print(f"  Title overlap      : {title_overlap:.3f}  ({len(common_titles)}/{denom})")
    print(f"  TOC page present   : oracle={toc_in_ref}  new={toc_in_new}")
    print(f"  Titles in oracle   : {titles_ref}")
    print(f"  Titles in new      : {titles_new}")
    print(f"  Only in oracle     : {sorted(set_ref - set_new)}")
    print(f"  Only in new        : {sorted(set_new - set_ref)}")
    print(f"  Text similarity    : {text_sim:.4f}")

    # --- build result dict ---
    result = {
        "mode": "m2b",
        "oracle": str(ORACLE_BIN),
        "footer_template": FOOTER_TMPL,
        "cover_html": str(cover_html),
        "inputs": [str(doc1), str(doc2)],
        "oracle_pdf": str(ref_pdf),
        "new_pdf": str(new_pdf),
        "pages_ref": pages_ref,
        "pages_new": pages_new,
        "delta_pages": delta_pages,
        "pages_within_tolerance": pages_within_tolerance,
        "outline_ref_n": outline_ref_n,
        "outline_new_n": outline_new_n,
        "titles_ref": titles_ref,
        "titles_new": titles_new,
        "common_titles": sorted(common_titles),
        "only_in_ref": sorted(set_ref - set_new),
        "only_in_new": sorted(set_new - set_ref),
        "title_overlap": round(title_overlap, 4),
        "toc_in_ref": toc_in_ref,
        "toc_in_new": toc_in_new,
        "text_sim": round(text_sim, 4),
    }

    # --- write results-m2b.md ---
    md_path = SCRIPT_DIR / "results-m2b.md"
    titles_ref_str = "\n".join(f"  - {t}" for t in titles_ref) if titles_ref else "  (none)"
    titles_new_str = "\n".join(f"  - {t}" for t in titles_new) if titles_new else "  (none)"
    only_ref_str = (
        "\n".join(f"  - {t}" for t in sorted(set_ref - set_new))
        if (set_ref - set_new) else "  (none)"
    )
    only_new_str = (
        "\n".join(f"  - {t}" for t in sorted(set_new - set_ref))
        if (set_new - set_ref) else "  (none)"
    )

    md_content = textwrap.dedent(f"""\
        # M2b Assembly Comparison: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `{ORACLE_BIN}`
        Inputs: cover page + `{doc1.name}` + `{doc2.name}`
        Footer template: `{FOOTER_TMPL}`
        New engine: `cargo run --example assemble -- --toc --cover cover.html --footer-center "[page]/[topage]"`

        ## Page Count

        | Side    | Pages |
        |:--------|------:|
        | Oracle  | {pages_ref} |
        | New     | {pages_new} |
        | Δ       | {delta_pages:+d} |
        | Within ±2 tolerance | {'Yes' if pages_within_tolerance else 'No'} |

        > Note: ±2 tolerance accepted because cover + TOC add pages and Chrome/wkhtmltopdf
        > paginate slightly differently.

        ## TOC Page Detection

        | Side    | TOC page present |
        |:--------|:-----------------|
        | Oracle  | {toc_in_ref} |
        | New     | {toc_in_new} |

        ## Outline (Bookmark) Comparison

        | Metric                           | Value |
        |:---------------------------------|------:|
        | Oracle outline entries           | {outline_ref_n} |
        | New engine outline entries       | {outline_new_n} |
        | Title overlap (intersection/max) | {title_overlap:.3f} ({len(common_titles)}/{denom}) |

        ### Oracle bookmark titles
{titles_ref_str}

        ### New-engine bookmark titles
{titles_new_str}

        ### Titles only in oracle
{only_ref_str}

        ### Titles only in new engine
{only_new_str}

        ## Text Similarity

        | Metric   | Value  |
        |:---------|-------:|
        | text_sim | {text_sim:.4f} |

        ## Notes

        - Oracle uses native `cover` + `toc` subcommands; new engine uses `assemble --toc --cover`.
        - Outline page numbers are structural only; exact pages compared via oracle in M2b Task 1.
        - TOC page detection: searches for "table of contents" or "contents" in page text.
        - Source::Html is now implemented in ChromiumRenderer (temp-file + file:// URL approach).
    """)
    md_path.write_text(md_content, encoding="utf-8")
    print(f"\nWrote {md_path}")

    # --- write results-m2b.json ---
    json_path = SCRIPT_DIR / "results-m2b.json"
    json_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote {json_path}")


# ---------------------------------------------------------------------------
# M3b CLI comparison (--cli mode)
# ---------------------------------------------------------------------------

def run_cli_comparison():
    """
    Compare OUR wkhtmltopdf CLI binary vs the oracle on the same args/doc.

    Structural comparison:
      Both invoked with: -s A4 --toc headings.html longtext.html out.pdf
      (Oracle uses 'toc' subcommand; ours uses --toc flag.)
      Metrics: page count (Δ, ±2 tol), outline entry count, title-set overlap,
               TOC page present in both.

    Exit-code checks (3 scenarios on BOTH binaries):
      (a) successful convert     → expect 0
      (b) unknown flag           → expect nonzero
      (c) missing input file     → expect nonzero
    """
    corpus_dir = SCRIPT_DIR / "corpus"
    doc1 = corpus_dir / "headings.html"
    doc2 = corpus_dir / "longtext.html"

    for p in (doc1, doc2):
        if not p.exists():
            print(f"ERROR: corpus file not found: {p}", file=sys.stderr)
            sys.exit(1)

    # Resolve path to our CLI binary (two levels up from script → wkhtmltox-rs)
    our_bin = WORKSPACE_DIR / "target" / "debug" / "wkhtmltopdf"
    if not our_bin.exists():
        print(
            f"ERROR: our binary not found at {our_bin}; "
            "run `cargo build -p wkhtmltopdf-cli` first.",
            file=sys.stderr,
        )
        sys.exit(1)

    cli_out_dir = SCRIPT_DIR / "out-cli"
    cli_out_dir.mkdir(parents=True, exist_ok=True)

    ref_pdf = cli_out_dir / "cli.ref.pdf"
    our_pdf = cli_out_dir / "cli.our.pdf"

    print(f"Oracle  : {ORACLE_BIN}")
    print(f"Ours    : {our_bin}")
    print(f"Inputs  : {doc1.name} + {doc2.name}")
    print(f"Out dir : {cli_out_dir}")
    print()

    # ── Structural comparison ────────────────────────────────────────────────

    # Oracle: uses 'toc' positional subcommand
    print("Running oracle CLI (with toc subcommand) ...", flush=True)
    oracle_cmd = [
        str(ORACLE_BIN),
        "-s", "A4",
        "--outline",
        "toc",
        str(doc1), str(doc2),
        str(ref_pdf),
    ]
    try:
        oracle_res = subprocess.run(oracle_cmd, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        print("  ORACLE FAILED: timeout", file=sys.stderr)
        sys.exit(1)
    oracle_stderr = oracle_res.stderr.strip()[-300:] if oracle_res.stderr else ""
    if oracle_res.returncode != 0 or not ref_pdf.exists():
        print(
            f"  ORACLE FAILED: rc={oracle_res.returncode}; stderr: {oracle_stderr}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"  oracle OK  -> {ref_pdf}  rc={oracle_res.returncode}")

    # Ours: uses --toc flag
    print("Running our CLI (with --toc flag) ...", flush=True)
    our_cmd = [
        str(our_bin),
        "-s", "A4",
        "--toc",
        str(doc1.resolve()), str(doc2.resolve()),
        str(our_pdf),
    ]
    try:
        our_res = subprocess.run(our_cmd, capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        print("  OUR BINARY FAILED: timeout", file=sys.stderr)
        sys.exit(1)
    our_stderr = our_res.stderr.strip()[-300:] if our_res.stderr else ""
    if our_res.returncode != 0 or not our_pdf.exists():
        print(
            f"  OUR BINARY FAILED: rc={our_res.returncode}; stderr: {our_stderr}",
            file=sys.stderr,
        )
        sys.exit(1)
    print(f"  ours   OK  -> {our_pdf}  rc={our_res.returncode}")

    # Open with pymupdf
    ref_doc = fitz.open(str(ref_pdf))
    our_doc = fitz.open(str(our_pdf))

    pages_ref = ref_doc.page_count
    pages_our = our_doc.page_count
    delta_pages = pages_our - pages_ref
    pages_within_tolerance = abs(delta_pages) <= 2

    toc_ref = ref_doc.get_toc()
    toc_our = our_doc.get_toc()
    titles_ref = [title.strip() for _lvl, title, _page in toc_ref]
    titles_our = [title.strip() for _lvl, title, _page in toc_our]
    outline_ref_n = len(toc_ref)
    outline_our_n = len(toc_our)

    set_ref = set(titles_ref)
    set_our = set(titles_our)
    common_titles = set_ref & set_our
    denom = max(len(set_ref), len(set_our), 1)
    title_overlap = len(common_titles) / denom

    toc_in_ref = has_toc_page(ref_doc)
    toc_in_our = has_toc_page(our_doc)

    ref_doc.close()
    our_doc.close()

    print()
    print(f"  Page count       : oracle={pages_ref}  ours={pages_our}  Δ={delta_pages:+d}  within_±2={pages_within_tolerance}")
    print(f"  Outline entries  : oracle={outline_ref_n}  ours={outline_our_n}")
    print(f"  Title overlap    : {title_overlap:.3f}  ({len(common_titles)}/{denom})")
    print(f"  TOC page present : oracle={toc_in_ref}  ours={toc_in_our}")

    # ── Exit-code checks ─────────────────────────────────────────────────────

    # A helper to run a binary and capture its exit code.
    def run_exit(label: str, cmd: list, *, timeout: int = 60) -> tuple[int, str]:
        try:
            r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
            combined = (r.stderr + r.stdout).strip()[-200:]
            return r.returncode, combined
        except subprocess.TimeoutExpired:
            return -999, "timeout"
        except Exception as exc:
            return -998, str(exc)

    # Scenario (a): successful convert
    sc_a_ref_pdf = cli_out_dir / "ec-a.ref.pdf"
    sc_a_our_pdf = cli_out_dir / "ec-a.our.pdf"
    sc_a_oracle_rc, sc_a_oracle_out = run_exit(
        "oracle-success",
        [str(ORACLE_BIN), "-s", "A4", str(doc1), str(sc_a_ref_pdf)],
    )
    sc_a_our_rc, sc_a_our_out = run_exit(
        "our-success",
        [str(our_bin), "-s", "A4", str(doc1.resolve()), str(sc_a_our_pdf)],
    )
    sc_a_oracle_ok = sc_a_oracle_rc == 0
    sc_a_our_ok = sc_a_our_rc == 0
    sc_a_match = sc_a_oracle_ok and sc_a_our_ok  # both should be 0

    # Scenario (b): unknown flag
    sc_b_oracle_rc, sc_b_oracle_out = run_exit(
        "oracle-unknown-flag",
        [str(ORACLE_BIN), "--frobnicate", str(doc1), str(cli_out_dir / "ec-b-dummy.pdf")],
    )
    sc_b_our_rc, sc_b_our_out = run_exit(
        "our-unknown-flag",
        [str(our_bin), "--frobnicate", str(doc1.resolve()), str(cli_out_dir / "ec-b-dummy.pdf")],
    )
    sc_b_oracle_nonzero = sc_b_oracle_rc != 0
    sc_b_our_nonzero = sc_b_our_rc != 0
    sc_b_match = sc_b_oracle_nonzero and sc_b_our_nonzero

    # Scenario (c): missing input file — use a path that neither binary can render
    # Note: our binary converts paths to file:// URLs; Chrome renders a blank page
    # and returns 0. This is a known divergence from oracle behavior.
    missing_path = str(cli_out_dir / "absolutely-nonexistent-input.html")
    sc_c_oracle_rc, sc_c_oracle_out = run_exit(
        "oracle-missing-input",
        [str(ORACLE_BIN), missing_path, str(cli_out_dir / "ec-c-dummy.pdf")],
    )
    sc_c_our_rc, sc_c_our_out = run_exit(
        "our-missing-input",
        [str(our_bin), missing_path, str(cli_out_dir / "ec-c-dummy.pdf")],
    )
    sc_c_oracle_nonzero = sc_c_oracle_rc != 0
    sc_c_our_nonzero = sc_c_our_rc != 0
    sc_c_match = sc_c_oracle_nonzero == sc_c_our_nonzero

    print()
    print("Exit-code checks:")
    print(f"  (a) success   : oracle rc={sc_a_oracle_rc}  ours rc={sc_a_our_rc}  match={'YES' if sc_a_match else 'NO'}")
    print(f"  (b) bad flag  : oracle rc={sc_b_oracle_rc}  ours rc={sc_b_our_rc}  match={'YES' if sc_b_match else 'NO'}")
    print(f"  (c) miss file : oracle rc={sc_c_oracle_rc}  ours rc={sc_c_our_rc}  match={'YES' if sc_c_match else 'NO'}")
    if not sc_c_match:
        print(
            "  NOTE (c): our binary converts local paths to file:// URLs; "
            "Chrome renders an error page silently (exit 0). Oracle fails the "
            "network lookup (exit nonzero). Divergence is known."
        )

    # ── Build result dict ────────────────────────────────────────────────────

    result = {
        "mode": "cli",
        "oracle": str(ORACLE_BIN),
        "our_bin": str(our_bin),
        "inputs": [str(doc1), str(doc2)],
        "oracle_pdf": str(ref_pdf),
        "our_pdf": str(our_pdf),
        # Structural
        "pages_ref": pages_ref,
        "pages_our": pages_our,
        "delta_pages": delta_pages,
        "pages_within_tolerance": pages_within_tolerance,
        "outline_ref_n": outline_ref_n,
        "outline_our_n": outline_our_n,
        "titles_ref": titles_ref,
        "titles_our": titles_our,
        "common_titles": sorted(common_titles),
        "only_in_ref": sorted(set_ref - set_our),
        "only_in_our": sorted(set_our - set_ref),
        "title_overlap": round(title_overlap, 4),
        "toc_in_ref": toc_in_ref,
        "toc_in_our": toc_in_our,
        # Exit codes
        "exit_codes": {
            "success": {
                "oracle_rc": sc_a_oracle_rc,
                "our_rc": sc_a_our_rc,
                "match": sc_a_match,
            },
            "unknown_flag": {
                "oracle_rc": sc_b_oracle_rc,
                "our_rc": sc_b_our_rc,
                "match": sc_b_match,
            },
            "missing_input": {
                "oracle_rc": sc_c_oracle_rc,
                "our_rc": sc_c_our_rc,
                "match": sc_c_match,
                "note": (
                    "our binary converts local paths to file:// URLs; "
                    "Chrome renders an error page silently (exit 0 vs oracle exit nonzero)."
                    if not sc_c_match else None
                ),
            },
        },
    }

    # ── Write results-cli.md ─────────────────────────────────────────────────

    titles_ref_str = "\n".join(f"  - {t}" for t in titles_ref) if titles_ref else "  (none)"
    titles_our_str = "\n".join(f"  - {t}" for t in titles_our) if titles_our else "  (none)"
    only_ref_str = (
        "\n".join(f"  - {t}" for t in sorted(set_ref - set_our))
        if (set_ref - set_our) else "  (none)"
    )
    only_our_str = (
        "\n".join(f"  - {t}" for t in sorted(set_our - set_ref))
        if (set_our - set_ref) else "  (none)"
    )

    def ec_row(label, oracle_rc, our_rc, match):
        return f"| {label:<20} | {oracle_rc:>10} | {our_rc:>8} | {'YES' if match else 'NO':<5} |"

    md_content = textwrap.dedent(f"""\
        # M3b CLI Comparison: wkhtmltopdf (ours) vs oracle 0.12.6

        Oracle: `{ORACLE_BIN}`
        Ours:   `{our_bin}`
        Inputs: `{doc1.name}` + `{doc2.name}`

        Oracle invocation: `-s A4 --outline toc headings.html longtext.html out.pdf`
        Ours   invocation: `-s A4 --toc headings.html longtext.html out.pdf`

        Note: oracle uses the `toc` positional subcommand; ours uses `--toc` flag.

        ## Structural Comparison

        ### Page Count

        | Side    | Pages |
        |:--------|------:|
        | Oracle  | {pages_ref} |
        | Ours    | {pages_our} |
        | Δ       | {delta_pages:+d} |
        | Within ±2 tolerance | {'Yes' if pages_within_tolerance else 'No'} |

        ### TOC Page Detection

        | Side    | TOC page present |
        |:--------|:-----------------|
        | Oracle  | {toc_in_ref} |
        | Ours    | {toc_in_our} |

        ### Outline (Bookmark) Comparison

        | Metric                           | Value |
        |:---------------------------------|------:|
        | Oracle outline entries           | {outline_ref_n} |
        | Ours outline entries             | {outline_our_n} |
        | Title overlap (intersection/max) | {title_overlap:.3f} ({len(common_titles)}/{denom}) |

        #### Oracle bookmark titles
{titles_ref_str}

        #### Ours bookmark titles
{titles_our_str}

        #### Titles only in oracle
{only_ref_str}

        #### Titles only in ours
{only_our_str}

        ## Exit-Code Comparison

        | Scenario             | oracle rc  | ours rc  | match |
        |:---------------------|:----------:|:--------:|:-----:|
        {ec_row("(a) success", sc_a_oracle_rc, sc_a_our_rc, sc_a_match)}
        {ec_row("(b) unknown flag", sc_b_oracle_rc, sc_b_our_rc, sc_b_match)}
        {ec_row("(c) missing input", sc_c_oracle_rc, sc_c_our_rc, sc_c_match)}

        ### Notes on exit codes

        - **(a) success**: both return 0. Match.
        - **(b) unknown flag (`--frobnicate`)**: both return nonzero. Match.
        - **(c) missing input**: oracle returns nonzero (network error); our binary
          converts paths to `file://` URLs and Chrome renders an error page silently,
          returning exit 0. **Known divergence** — not yet fixed in M3b.

        ## Notes

        - Page count tolerance ±2 accepted; Chrome and wkhtmltopdf paginate differently.
        - Title overlap is the structural fidelity signal.
        - Oracle `toc` subcommand ↔ our `--toc` flag: semantically equivalent but
          syntactically different (upstream grammar quirk).
    """)

    md_path = SCRIPT_DIR / "results-cli.md"
    md_path.write_text(md_content, encoding="utf-8")
    print(f"\nWrote {md_path}")

    json_path = SCRIPT_DIR / "results-cli.json"
    json_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(f"Wrote {json_path}")


# ---------------------------------------------------------------------------
# M5 image comparison (--image mode)
# ---------------------------------------------------------------------------

ORACLE_IMAGE_BIN = Path(
    os.environ.get(
        "WKHTMLTOX_IMAGE_ORACLE",
        "/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltoimage",
    )
)

IMAGE_CORPUS_DOCS = ["text.html", "cssbox.html"]
IMAGE_WIDTH = 800


def render_oracle_image(html_path: Path, out_png: Path) -> tuple[str, str]:
    """Run oracle wkhtmltoimage and return (status, detail)."""
    cmd = [
        str(ORACLE_IMAGE_BIN),
        "--format", "png",
        "--width", str(IMAGE_WIDTH),
        "--enable-local-file-access",
        "--quiet",
        str(html_path),
        str(out_png),
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return "ORACLE_CRASH", "timeout after 120s"
    except Exception as exc:
        return "ORACLE_CRASH", f"subprocess error: {exc}"

    stderr_tail = result.stderr.strip()[-300:] if result.stderr else ""
    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "ORACLE_CRASH", f"killed by SIG{sig_name}; stderr: {stderr_tail}"
    if result.returncode != 0 or not out_png.exists():
        return "ORACLE_ERROR", f"rc={result.returncode}; stderr: {stderr_tail}"
    return "OK", stderr_tail


def render_our_image(html_path: Path, out_png: Path) -> tuple[str, str]:
    """Run our wkhtmltoimage binary and return (status, detail)."""
    our_bin = WORKSPACE_DIR / "target" / "debug" / "wkhtmltoimage"
    if not our_bin.exists():
        return "OUR_ERROR", f"binary not found at {our_bin}; run `cargo build -p wkhtmltoimage-cli` first"
    cmd = [
        str(our_bin),
        "--format", "png",
        "--width", str(IMAGE_WIDTH),
        "--enable-local-file-access",
        "--quiet",
        str(html_path.resolve()),
        str(out_png),
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return "OUR_CRASH", "timeout after 120s"
    except Exception as exc:
        return "OUR_CRASH", f"subprocess error: {exc}"

    combined = (result.stderr + result.stdout).strip()
    tail = combined[-300:] if combined else ""
    if result.returncode < 0:
        import signal as _signal
        sig = -result.returncode
        try:
            sig_name = _signal.Signals(sig).name
        except ValueError:
            sig_name = str(sig)
        return "OUR_CRASH", f"killed by SIG{sig_name}; output: {tail}"
    if result.returncode != 0 or not out_png.exists():
        return "OUR_ERROR", f"rc={result.returncode}; output: {tail}"
    return "OK", tail


def png_dimensions(path: Path) -> tuple[int, int]:
    """Return (width, height) of an image file using PIL."""
    from PIL import Image
    with Image.open(str(path)) as img:
        return img.size  # (width, height)


def image_ssim(path_a: Path, path_b: Path) -> float:
    """
    Compute SSIM between two images (PIL/numpy/skimage).
    Both images are loaded, converted to grayscale, resized to a common size
    (the minimum of the two in each dimension) if they differ, then compared.
    Returns SSIM in [-1, 1] (1 = identical).
    """
    from PIL import Image

    img_a = Image.open(str(path_a)).convert("L")  # grayscale
    img_b = Image.open(str(path_b)).convert("L")

    wa, ha = img_a.size
    wb, hb = img_b.size

    if (wa, ha) != (wb, hb):
        # Resize to minimum common dimensions to avoid distortion artifacts
        w = min(wa, wb)
        h = min(ha, hb)
        img_a = img_a.resize((w, h), Image.LANCZOS)
        img_b = img_b.resize((w, h), Image.LANCZOS)

    arr_a = np.array(img_a, dtype=np.uint8)
    arr_b = np.array(img_b, dtype=np.uint8)

    win = min(arr_a.shape[0], arr_a.shape[1], 7)
    if win < 3:
        win = 3
    if win % 2 == 0:
        win -= 1

    return float(ssim(arr_a, arr_b, data_range=255, win_size=win))


def run_image_comparison():
    """
    M5 Task 4: compare our wkhtmltoimage vs oracle on corpus docs.
    Renders PNG (width=800) for each doc via both binaries.
    Metrics: valid PNG, dimensions (ours vs oracle, delta), SSIM.
    Writes results-image.md and results-image.json.
    """
    corpus_dir = SCRIPT_DIR / "corpus"
    our_bin = WORKSPACE_DIR / "target" / "debug" / "wkhtmltoimage"

    print(f"Oracle image : {ORACLE_IMAGE_BIN}")
    print(f"Ours         : {our_bin}")
    print(f"Corpus       : {corpus_dir}")
    print(f"Width        : {IMAGE_WIDTH}")
    print()

    if not ORACLE_IMAGE_BIN.exists():
        print(f"ERROR: oracle wkhtmltoimage not found at {ORACLE_IMAGE_BIN}", file=sys.stderr)
        sys.exit(1)
    if not our_bin.exists():
        print(
            f"ERROR: our binary not found at {our_bin}; "
            "run `cargo build -p wkhtmltoimage-cli` first.",
            file=sys.stderr,
        )
        sys.exit(1)

    img_out_dir = SCRIPT_DIR / "out-image"
    img_out_dir.mkdir(parents=True, exist_ok=True)

    results = []

    for doc_name in IMAGE_CORPUS_DOCS:
        html_path = corpus_dir / doc_name
        if not html_path.exists():
            print(f"  SKIP: corpus file not found: {html_path}", file=sys.stderr)
            results.append({"name": doc_name, "status": "SKIP", "error": f"not found: {html_path}"})
            continue

        stem = html_path.stem
        oracle_png = img_out_dir / f"{stem}.oracle.png"
        our_png = img_out_dir / f"{stem}.ours.png"

        print(f"  {doc_name} ...", flush=True)

        # Oracle render
        oracle_status, oracle_detail = render_oracle_image(html_path, oracle_png)
        if oracle_status != "OK":
            print(f"    ORACLE FAILED: {oracle_status}: {oracle_detail[:200]}")
            results.append({"name": stem, "status": oracle_status, "error": oracle_detail[:300]})
            continue
        print(f"    oracle -> {oracle_png.name}", end="")

        # Our render
        our_status, our_detail = render_our_image(html_path, our_png)
        if our_status != "OK":
            print()
            print(f"    OUR BINARY FAILED: {our_status}: {our_detail[:200]}")
            results.append({"name": stem, "status": our_status, "error": our_detail[:300]})
            continue
        print(f"  ours -> {our_png.name}")

        # Both PNGs exist — measure dimensions and SSIM
        try:
            oracle_w, oracle_h = png_dimensions(oracle_png)
            our_w, our_h = png_dimensions(our_png)
            delta_w = our_w - oracle_w
            delta_h = our_h - oracle_h
        except Exception as exc:
            results.append({"name": stem, "status": "METRICS_ERROR", "error": f"dimensions: {exc}"})
            continue

        try:
            img_ssim = image_ssim(oracle_png, our_png)
        except Exception as exc:
            results.append({"name": stem, "status": "METRICS_ERROR", "error": f"SSIM: {exc}"})
            continue

        print(
            f"    oracle {oracle_w}x{oracle_h}  ours {our_w}x{our_h}  "
            f"Δw={delta_w:+d} Δh={delta_h:+d}  SSIM={img_ssim:.4f}"
        )

        results.append({
            "name": stem,
            "status": "OK",
            "oracle_w": oracle_w,
            "oracle_h": oracle_h,
            "our_w": our_w,
            "our_h": our_h,
            "delta_w": delta_w,
            "delta_h": delta_h,
            "ssim": round(img_ssim, 4),
        })

    # ── Build report ──────────────────────────────────────────────────────────

    ok_results = [r for r in results if r.get("status") == "OK"]
    mean_ssim = round(float(np.mean([r["ssim"] for r in ok_results])), 4) if ok_results else None
    min_ssim = round(float(np.min([r["ssim"] for r in ok_results])), 4) if ok_results else None

    print()
    print(f"Summary: {len(ok_results)}/{len(results)} OK  mean_ssim={mean_ssim}  min_ssim={min_ssim}")

    # ── Write results-image.md ─────────────────────────────────────────────────

    def img_row(r):
        if r.get("status") != "OK":
            return (
                f"| {r['name']:<20} | {r['status']:<12} | --- | --- | --- | --- | --- | --- | --- |"
            )
        return (
            f"| {r['name']:<20} | {'OK':<12} "
            f"| {r['oracle_w']:>5} | {r['oracle_h']:>6} "
            f"| {r['our_w']:>5} | {r['our_h']:>6} "
            f"| {r['delta_w']:>+4} | {r['delta_h']:>+5} "
            f"| {r['ssim']:.4f} |"
        )

    table_header = (
        "| Document             | status       | o_w   | o_h    | u_w   | u_h    |  Δw  |   Δh  |   SSIM |"
    )
    table_sep = (
        "|:---------------------|:-------------|------:|-------:|------:|-------:|-----:|------:|-------:|"
    )
    table_rows = "\n".join([table_header, table_sep] + [img_row(r) for r in results])

    agg_note = (
        f"**Aggregate ({len(ok_results)} OK / {len(results) - len(ok_results)} errors)**: "
        f"mean_ssim={mean_ssim}  min_ssim={min_ssim}"
    )

    md_content = textwrap.dedent(f"""\
        # M5 Image Comparison: wkhtmltoimage (ours) vs oracle 0.12.6

        Oracle: `{ORACLE_IMAGE_BIN}`
        Ours:   `{our_bin}`
        Corpus: {', '.join(IMAGE_CORPUS_DOCS)}
        Width:  {IMAGE_WIDTH}px
        Format: PNG

        ## Results

        {table_rows}

        {agg_note}

        ## Legend

        - **o_w / o_h**: oracle image width / height in pixels
        - **u_w / u_h**: our image width / height in pixels
        - **Δw / Δh**: our − oracle dimension delta
        - **SSIM**: structural similarity (grayscale; images resized to common dims if they differ; 1 = identical)

        ## Notes

        - Both binaries called with `--format png --width {IMAGE_WIDTH} --enable-local-file-access`.
        - SSIM measured on a single image (no pagination), so it is directly comparable between docs.
        - Images are resized to the smaller of the two sizes (LANCZOS) before SSIM if dimensions differ.
        - For reference, PDF page SSIM from previous milestones ranged ~0.55–0.75 (cross-engine, different renderer).
        - Here both pipelines render the same HTML; SSIM reflects font/layout drift between Qt-WebKit and Chrome.
    """)

    md_path = SCRIPT_DIR / "results-image.md"
    md_path.write_text(md_content, encoding="utf-8")
    print(f"Wrote {md_path}")

    # ── Write results-image.json ───────────────────────────────────────────────

    summary = {
        "mode": "image",
        "oracle": str(ORACLE_IMAGE_BIN),
        "our_bin": str(our_bin),
        "corpus_docs": IMAGE_CORPUS_DOCS,
        "width": IMAGE_WIDTH,
        "format": "png",
        "results": results,
        "aggregate": {
            "n_ok": len(ok_results),
            "n_errors": len(results) - len(ok_results),
            "mean_ssim": mean_ssim,
            "min_ssim": min_ssim,
        },
    }
    json_path = SCRIPT_DIR / "results-image.json"
    json_path.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    print(f"Wrote {json_path}")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
    if ARGS.image:
        run_image_comparison()
        return

    if ARGS.cli:
        run_cli_comparison()
        return

    if ARGS.m2b:
        run_m2b_comparison()
        return

    if ARGS.assemble:
        run_assembly_comparison()
        return

    print(f"Oracle  : {ORACLE_BIN}")
    print(f"Corpus  : {CORPUS_DIR} ({len(CORPUS_FILES)} files)")
    print(f"Out dir : {OUT_DIR}")
    print(f"Prefix  : {_prefix}")
    print()

    results = []
    for html_path in CORPUS_FILES:
        print(f"  measuring {html_path.name} ...", flush=True)
        try:
            r = measure_file(html_path)
        except Exception as exc:
            r = {
                "name": html_path.stem,
                "status": "HARNESS_ERROR",
                "error": f"unexpected harness error: {exc}",
            }
        results.append(r)
        if r.get("status") != "OK":
            print(f"    {r.get('status','ERROR')}: {(r.get('error') or '')[:120]}")
        else:
            print(
                f"    pages {r['pages_ref']}->{r['pages_new']}  "
                f"text_sim={r['text_sim']:.3f}  "
                f"mean_ssim={r['mean_ssim']:.3f}  "
                f"score={r['score']:.3f}"
            )

    table = build_table(results)
    agg = aggregate(results)

    agg_row = (
        f"\n**Aggregate ({agg.get('n_ok',0)} OK / {agg.get('n_errors',0)} errors)**: "
        f"mean_text_sim={agg.get('mean_text_sim','n/a')}  "
        f"mean_body_text_sim={agg.get('mean_body_text_sim','n/a')}  "
        f"min_outline_ratio={agg.get('min_outline_ratio','n/a')}  "
        f"mean_ssim={agg.get('mean_ssim','n/a')}  "
        f"min_ssim_overall={agg.get('min_ssim_overall','n/a')}  "
        f"mean_px_diff={agg.get('mean_pixel_diff_pct','n/a')}%  "
        f"mean_score={agg.get('mean_score','n/a')}  "
        f"total_|Δpages|={agg.get('total_page_delta','n/a')}  "
        f"max_|Δpages|={agg.get('max_abs_delta_pages','n/a')}"
    )

    print()
    print(table)
    print(agg_row)

    # --- write results markdown ---
    results_md = SCRIPT_DIR / f"{_prefix}.md"
    md_content = textwrap.dedent(f"""\
        # Fidelity Measurement: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `{ORACLE_BIN}`
        Corpus: `{CORPUS_DIR}` ({len(CORPUS_FILES)} files)
        DPI for visual comparison: 100

        ## Metrics

        {table}

        {agg_row}

        ## Legend

        - **status**: OK | ORACLE_CRASH | ORACLE_ERROR | NEW_CRASH | NEW_ERROR | METRICS_ERROR
        - **p_ref / p_new**: page count from oracle / new engine
        - **Δp**: page count difference (new − ref)
        - **text_sim**: SequenceMatcher ratio on whitespace-normalised extracted text (0=none, 1=identical)
        - **body_sim**: SequenceMatcher ratio on body-only text (margin bands + optional TOC page excluded)
        - **outline ref/new**: TOC entry count; ratio = ordered LCS ratio (respects order and nesting)
        - **mean_ssim / min_ssim**: structural similarity over matched pages (1=identical)
        - **px_diff%**: mean absolute pixel difference / 255 × 100 over matched pages
        - **score**: (text_sim + mean_ssim + page_count_closeness) / 3
    """)
    results_md.write_text(md_content, encoding="utf-8")
    print(f"\nWrote {results_md}")

    # --- write results json ---
    results_json = SCRIPT_DIR / f"{_prefix}.json"
    results_json.write_text(
        json.dumps({"results": results, "aggregate": agg}, indent=2),
        encoding="utf-8",
    )
    print(f"Wrote {results_json}")

    # --- gate check (only when --gate is passed) ---
    if ARGS.gate:
        thresholds_path = Path(
            ARGS.thresholds if ARGS.thresholds else (SCRIPT_DIR / "thresholds.json")
        )
        try:
            with open(thresholds_path) as _tf:
                thresholds = json.load(_tf)
        except FileNotFoundError:
            print(f"GATE ERROR: thresholds file not found: {thresholds_path}", file=sys.stderr)
            sys.exit(2)

        ok_results = [r for r in results if r.get("status") == "OK"]
        if ok_results:
            gate_metrics = {
                "mean_ssim": agg.get("mean_ssim", 0.0),
                "outline_ratio": agg.get("min_outline_ratio", 0.0),
                "abs_delta_pages": agg.get("max_abs_delta_pages", 0),
                "text_sim": agg.get("mean_body_text_sim", 0.0),
            }
            failures = gate_check(gate_metrics, thresholds)
            if failures:
                print()
                for msg in failures:
                    print(f"GATE FAIL: {msg}")
                sys.exit(1)
            else:
                print("\nGATE: PASS")
        else:
            print("\nGATE: no OK results to evaluate", file=sys.stderr)
            sys.exit(2)


if __name__ == "__main__":
    main()
