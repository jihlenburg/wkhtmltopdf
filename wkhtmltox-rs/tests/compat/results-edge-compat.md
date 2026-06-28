        # Fidelity Measurement: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
        Corpus: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/tests/compat/edge` (16 files)
        DPI for visual comparison: 100

        ## Metrics

        | Document                     | status        | p_ref | p_new |  Δp | text_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |
|:-----------------------------|:--------------|------:|------:|----:|---------:|:----------------|----------:|---------:|---------:|-------:|
| 01-break-css3                | OK            |  1 |  3 |  +2 | 1.0000  |  3/3  (1.00) | 0.7311 | 0.7311 |  5.04% | 0.5770 |
| 02-break-legacy              | OK            |  3 |  3 |  +0 | 1.0000  |  3/3  (1.00) | 0.8559 | 0.8558 |  2.87% | 0.9520 |
| 03-breakinside-avoid         | OK            |  2 |  2 |  +0 | 1.0000  |  3/3  (1.00) | 0.6103 | 0.5009 |  9.22% | 0.8701 |
| 04-widows-orphans            | OK            |  1 |  2 |  +1 | 1.0000  |  0/0  (0.00) | 0.7001 | 0.7001 |  6.04% | 0.5667 |
| 05-table-thead-repeat        | OK            |  2 |  2 |  +0 | 0.6089  |  0/0  (0.00) | 0.5963 | 0.4763 |  4.26% | 0.7351 |
| 06-grid                      | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8782 | 0.8782 |  3.82% | 0.9594 |
| 07-flexbox-gap               | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.9170 | 0.9170 |  1.32% | 0.9723 |
| 08-cssvars-calc              | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (0.00) | 0.9393 | 0.9393 |  1.43% | 0.9798 |
| 09-justify-hyphens           | OK            |  1 |  1 |  +0 | 0.9973  |  1/1  (1.00) | 0.4778 | 0.4778 | 11.37% | 0.8250 |
| 10-rtl-bidi                  | OK            |  1 |  1 |  +0 | 0.7337  |  1/1  (1.00) | 0.9683 | 0.9683 |  0.62% | 0.9007 |
| 11-cjk-emoji                 | OK            |  1 |  1 |  +0 | 0.9220  |  1/1  (1.00) | 0.9395 | 0.9395 |  1.48% | 0.9538 |
| 12-visual-effects            | OK            |  1 |  1 |  +0 | 0.5750  |  1/1  (1.00) | 0.9356 | 0.9356 |  3.07% | 0.8369 |
| 13-tall-image-objectfit      | OK            |  2 |  3 |  +1 | 1.0000  |  1/1  (1.00) | 0.9545 | 0.9492 |  3.84% | 0.8182 |
| 14-print-media-coloradjust   | OK            |  1 |  1 |  +0 | 0.9058  |  1/1  (1.00) | 0.9220 | 0.9220 |  5.33% | 0.9426 |
| 15-positioned-abs-fixed      | OK            |  2 |  2 |  +0 | 0.8765  |  1/1  (1.00) | 0.9797 | 0.9677 |  0.54% | 0.9521 |
| 16-malformed-nowrap          | OK            |  1 |  1 |  +0 | 0.9529  |  1/1  (0.00) | 0.9331 | 0.9331 |  1.57% | 0.9620 |


**Aggregate (16 OK / 0 errors)**: mean_text_sim=0.9108  mean_ssim=0.8337  min_ssim_overall=0.4763  mean_px_diff=3.864%  mean_score=0.8627  total_|Δpages|=4

        ## Legend

        - **status**: OK | ORACLE_CRASH | ORACLE_ERROR | NEW_CRASH | NEW_ERROR | METRICS_ERROR
        - **p_ref / p_new**: page count from oracle / new engine
        - **Δp**: page count difference (new − ref)
        - **text_sim**: SequenceMatcher ratio on whitespace-normalised extracted text (0=none, 1=identical)
        - **outline ref/new**: TOC entry count; ratio = intersection / max
        - **mean_ssim / min_ssim**: structural similarity over matched pages (1=identical)
        - **px_diff%**: mean absolute pixel difference / 255 × 100 over matched pages
        - **score**: (text_sim + mean_ssim + page_count_closeness) / 3
