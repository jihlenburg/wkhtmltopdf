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
ARGS = parser.parse_args()

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
else:
    _prefix = "results"

OUT_DIR = SCRIPT_DIR / f"out-{CORPUS_DIR.name}"
OUT_DIR.mkdir(parents=True, exist_ok=True)

ORACLE_BIN = Path("/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf")

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
    cmd = [
        "cargo", "run", "-q",
        "--example", "render",
        "-p", "wkhtmltox-render-chromium",
        "--",
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

def measure_file(html_path: Path) -> dict:
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
        outline_ref_n, outline_new_n, outline_ratio = outline_match(toc_ref, toc_new)

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
            f"| {r['name']:<28} | {status:<13} | -- | -- |  +0 | ------- | ------------- "
            f"| ----- | ----- | ----- | {err_short} |"
        )
    return (
        f"| {r['name']:<28} "
        f"| {'OK':<13} "
        f"| {r['pages_ref']:>2} "
        f"| {r['pages_new']:>2} "
        f"| {r['delta_pages']:>+3} "
        f"| {r['text_sim']:.4f}  "
        f"| {r['outline_ref']:>2}/{r['outline_new']:<2} ({r['outline_ratio']:.2f}) "
        f"| {r['mean_ssim']:.4f} "
        f"| {r['min_ssim']:.4f} "
        f"| {r['pixel_diff_pct']:>5.2f}% "
        f"| {r['score']:.4f} |"
    )


HEADER = (
    "| Document                     | status        | p_ref | p_new |  Δp | text_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |"
)
SEPARATOR = (
    "|:-----------------------------|:--------------|------:|------:|----:|---------:|:----------------|----------:|---------:|---------:|-------:|"
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
        "mean_ssim": round(float(np.mean([r["mean_ssim"] for r in ok])), 4),
        "min_ssim_overall": round(float(np.min([r["min_ssim"] for r in ok])), 4),
        "mean_pixel_diff_pct": round(float(np.mean([r["pixel_diff_pct"] for r in ok])), 3),
        "mean_score": round(float(np.mean([r["score"] for r in ok])), 4),
        "total_page_delta": int(np.sum([abs(r["delta_pages"]) for r in ok])),
    }


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
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
        f"mean_ssim={agg.get('mean_ssim','n/a')}  "
        f"min_ssim_overall={agg.get('min_ssim_overall','n/a')}  "
        f"mean_px_diff={agg.get('mean_pixel_diff_pct','n/a')}%  "
        f"mean_score={agg.get('mean_score','n/a')}  "
        f"total_|Δpages|={agg.get('total_page_delta','n/a')}"
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
        - **outline ref/new**: TOC entry count; ratio = intersection / max
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


if __name__ == "__main__":
    main()
