# Fidelity Measurement: compat ON (wk0126 UA-reset v1) vs baseline

Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
New engine: `wkhtmltox-render-chromium` with `--compat` (WK0126_UA_RESET injected)
DPI for visual comparison: 100
CSS version: v1 (body margin:8px, font-family:serif, font-size:16px, line-height:1.12, heading/block margins)

## Baseline corpus — per-doc metrics (compat ON)

| Document                     | status        | p_ref | p_new |  Δp | text_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |
|:-----------------------------|:--------------|------:|------:|----:|---------:|:----------------|----------:|---------:|---------:|-------:|
| cssbox                       | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8029 | 0.8029 |  4.27% | 0.9343 |
| fonts                        | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8621 | 0.8621 |  2.46% | 0.9540 |
| headings                     | OK            |  1 |  2 |  +1 | 1.0000  | 11/11 (1.00) | 0.6416 | 0.6416 |  7.84% | 0.5472 |
| longtext                     | OK            |  2 |  3 |  +1 | 1.0000  |  1/1  (1.00) | 0.4697 | 0.4369 | 11.07% | 0.6566 |
| table                        | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8359 | 0.8359 |  3.85% | 0.9453 |
| text                         | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.7920 | 0.7920 |  4.39% | 0.9307 |

**Aggregate (6 OK / 0 errors)**: mean_text_sim=1.0  mean_ssim=0.734  min_ssim_overall=0.4369  mean_px_diff=5.648%  mean_score=0.828  total_|Δpages|=2

## Baseline corpus — non-compat → compat delta

| Document       | SSIM before | SSIM after | ΔSSIM   | Δpage-drift |
|:---------------|------------:|-----------:|--------:|:-----------:|
| cssbox         | 0.8007      | 0.8029     | +0.0022 | 0           |
| fonts          | 0.8670      | 0.8621     | -0.0049 | 0           |
| headings       | 0.6094      | 0.6416     | +0.0322 | 0 (still +1)|
| longtext       | 0.4303      | 0.4697     | +0.0394 | 0 (still +1)|
| table          | 0.8370      | 0.8359     | -0.0011 | 0           |
| text           | 0.7601      | 0.7920     | +0.0319 | 0           |
| **AGGREGATE**  | **0.7174**  | **0.734**  | **+0.017** | unchanged=2 |

## Edge corpus — non-compat → compat delta

| Document                   | SSIM before | SSIM after | ΔSSIM   | Δpage-drift |
|:---------------------------|------------:|-----------:|--------:|:-----------:|
| 01-break-css3              | 0.7277      | 0.7311     | +0.0034 | 0 (still +2)|
| 02-break-legacy            | 0.8542      | 0.8559     | +0.0017 | 0           |
| 03-breakinside-avoid       | 0.6089      | 0.6103     | +0.0014 | 0           |
| 04-widows-orphans          | 0.6963      | 0.7001     | +0.0038 | 0 (still +1)|
| 05-table-thead-repeat      | 0.5976      | 0.5963     | -0.0013 | 0           |
| 06-grid                    | 0.8772      | 0.8782     | +0.0010 | 0           |
| 07-flexbox-gap             | 0.9182      | 0.9170     | -0.0012 | 0           |
| 08-cssvars-calc            | 0.9393      | 0.9393     | +0.0000 | 0           |
| 09-justify-hyphens         | 0.4708      | 0.4778     | +0.0070 | 0           |
| 10-rtl-bidi                | 0.9671      | 0.9683     | +0.0012 | 0           |
| 11-cjk-emoji               | 0.9427      | 0.9395     | -0.0032 | 0           |
| 12-visual-effects          | 0.9352      | 0.9356     | +0.0004 | 0           |
| 13-tall-image-objectfit    | 0.9543      | 0.9545     | +0.0002 | 0 (still +1)|
| 14-print-media-coloradjust | 0.9215      | 0.9220     | +0.0005 | 0           |
| 15-positioned-abs-fixed    | 0.9798      | 0.9797     | -0.0001 | 0           |
| 16-malformed-nowrap        | 0.9335      | 0.9331     | -0.0004 | 0           |
| **AGGREGATE**              | **0.8328**  | **0.8337** | **+0.0009** | unchanged=4 |

mean_text_sim: 0.9062 → 0.9108 (+0.0046)

## CSS Iteration Summary

- **v1** (chosen winner): `body{margin:8px;font-family:serif;font-size:16px;line-height:1.12}` + heading/block margins + print-color-adjust
- **v2** (tested, rejected): removed `margin:8px` from body, reduced line-height 1.12→1.10 — aggregate corpus SSIM 0.7331 < v1 0.734; table regressed -0.007

## Legend

- **status**: OK | ORACLE_CRASH | ORACLE_ERROR | NEW_CRASH | NEW_ERROR | METRICS_ERROR
- **p_ref / p_new**: page count from oracle / new engine
- **Δp**: page count difference (new − ref)
- **text_sim**: SequenceMatcher ratio on whitespace-normalised extracted text (0=none, 1=identical)
- **outline ref/new**: TOC entry count; ratio = intersection / max
- **mean_ssim / min_ssim**: structural similarity over matched pages (1=identical)
- **px_diff%**: mean absolute pixel difference / 255 × 100 over matched pages
- **score**: (text_sim + mean_ssim + page_count_closeness) / 3
