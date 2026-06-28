#!/usr/bin/env python3
# wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
"""
Fidelity-measurement harness: compare wkhtmltopdf 0.12.6 (oracle) vs
wkhtmltox-render-chromium (new engine) across the compat corpus.

Outputs:
  - table to stdout
  - tests/compat/results.md  (markdown table + notes)
  - tests/compat/results.json (raw numbers)
"""

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
# Paths
# ---------------------------------------------------------------------------
SCRIPT_DIR = Path(__file__).resolve().parent
CORPUS_DIR = SCRIPT_DIR / "corpus"
OUT_DIR = SCRIPT_DIR / "out"
WORKSPACE_DIR = SCRIPT_DIR.parent.parent  # wkhtmltox-rs/

ORACLE_BIN = Path("/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf")

OUT_DIR.mkdir(parents=True, exist_ok=True)

CORPUS_FILES = sorted(CORPUS_DIR.glob("*.html"))

# ---------------------------------------------------------------------------
# Render helpers
# ---------------------------------------------------------------------------

def render_oracle(html_path: Path, out_pdf: Path) -> tuple[bool, str]:
    """Run wkhtmltopdf and return (success, stderr)."""
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
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
    ok = (result.returncode == 0) and out_pdf.exists()
    return ok, result.stderr.strip()


def render_new(html_path: Path, out_pdf: Path) -> tuple[bool, str]:
    """Run the Chromium example and return (success, stderr)."""
    cmd = [
        "cargo", "run", "-q",
        "--example", "render",
        "-p", "wkhtmltox-render-chromium",
        "--",
        str(html_path.resolve()),
        str(out_pdf),
    ]
    result = subprocess.run(
        cmd, capture_output=True, text=True, timeout=120, cwd=str(WORKSPACE_DIR)
    )
    ok = (result.returncode == 0) and out_pdf.exists()
    stderr = (result.stderr + result.stdout).strip()
    return ok, stderr


# ---------------------------------------------------------------------------
# Metric helpers
# ---------------------------------------------------------------------------

def extract_text(doc: fitz.Document) -> str:
    """Extract and normalise text from all pages."""
    parts = []
    for page in doc:
        parts.append(page.get_text("text"))
    raw = " ".join(parts)
    # collapse whitespace
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
    # ratio of common elements
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

    result = {"name": name, "error": None}

    # --- oracle render ---
    ok_ref, err_ref = render_oracle(html_path, ref_pdf)
    if not ok_ref:
        result["error"] = f"oracle failed: {err_ref[:200]}"
        return result

    # --- new engine render ---
    ok_new, err_new = render_new(html_path, new_pdf)
    if not ok_new:
        result["error"] = f"new engine failed: {err_new[:200]}"
        return result

    # --- open with pymupdf ---
    try:
        ref_doc = fitz.open(str(ref_pdf))
        new_doc = fitz.open(str(new_pdf))
    except Exception as e:
        result["error"] = f"fitz open failed: {e}"
        return result

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
    if r.get("error"):
        return (
            f"| {r['name']:<12} | ERROR | ERROR | -- | ------- | ------------- "
            f"| ----- | ----- | ----- | {r['error'][:40]} |"
        )
    return (
        f"| {r['name']:<12} "
        f"| {r['pages_ref']:>5} "
        f"| {r['pages_new']:>5} "
        f"| {r['delta_pages']:>+3} "
        f"| {r['text_sim']:.4f}  "
        f"| {r['outline_ref']:>2}/{r['outline_new']:<2} ({r['outline_ratio']:.2f}) "
        f"| {r['mean_ssim']:.4f} "
        f"| {r['min_ssim']:.4f} "
        f"| {r['pixel_diff_pct']:>5.2f}% "
        f"| {r['score']:.4f} |"
    )


HEADER = (
    "| Document     | p_ref | p_new |  Δp | text_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |"
)
SEPARATOR = (
    "|:-------------|------:|------:|----:|---------:|:----------------|----------:|---------:|---------:|-------:|"
)


def build_table(results: list[dict]) -> str:
    lines = [HEADER, SEPARATOR]
    for r in results:
        lines.append(fmt_row(r))
    return "\n".join(lines)


def aggregate(results: list[dict]) -> dict:
    ok = [r for r in results if not r.get("error")]
    if not ok:
        return {"n": 0}
    return {
        "n": len(ok),
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
    print()

    results = []
    for html_path in CORPUS_FILES:
        print(f"  measuring {html_path.name} ...", flush=True)
        r = measure_file(html_path)
        results.append(r)
        if r.get("error"):
            print(f"    ERROR: {r['error']}")
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
        f"\n**Aggregate ({agg.get('n',0)} docs)**: "
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

    # --- write results.md ---
    results_md = SCRIPT_DIR / "results.md"
    md_content = textwrap.dedent(f"""\
        # Fidelity Measurement: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `{ORACLE_BIN}`
        Corpus: `{CORPUS_DIR}` ({len(CORPUS_FILES)} files)
        DPI for visual comparison: 100

        ## Metrics

        {table}

        {agg_row}

        ## Legend

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

    # --- write results.json ---
    results_json = SCRIPT_DIR / "results.json"
    results_json.write_text(
        json.dumps({"results": results, "aggregate": agg}, indent=2),
        encoding="utf-8",
    )
    print(f"Wrote {results_json}")


if __name__ == "__main__":
    main()
