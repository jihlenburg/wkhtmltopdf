        # Fidelity Measurement: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
        Corpus: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/tests/compat/edge` (16 files)
        DPI for visual comparison: 100

        ## Metrics

        | Document                     | status        | p_ref | p_new |  Δp | text_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |
|:-----------------------------|:--------------|------:|------:|----:|---------:|:----------------|----------:|---------:|---------:|-------:|
| 01-break-css3                | OK            |  1 |  3 |  +2 | 1.0000  |  3/3  (1.00) | 0.7277 | 0.7277 |  5.12% | 0.5759 |
| 02-break-legacy              | OK            |  3 |  3 |  +0 | 1.0000  |  3/3  (1.00) | 0.8542 | 0.8541 |  2.88% | 0.9514 |
| 03-breakinside-avoid         | OK            |  2 |  2 |  +0 | 1.0000  |  3/3  (1.00) | 0.6089 | 0.5036 |  9.26% | 0.8696 |
| 04-widows-orphans            | OK            |  1 |  2 |  +1 | 1.0000  |  0/0  (0.00) | 0.6963 | 0.6963 |  6.09% | 0.5654 |
| 05-table-thead-repeat        | OK            |  2 |  2 |  +0 | 0.6089  |  0/0  (0.00) | 0.5976 | 0.4748 |  4.27% | 0.7355 |
| 06-grid                      | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8772 | 0.8772 |  3.80% | 0.9591 |
| 07-flexbox-gap               | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (1.00) | 0.9182 | 0.9182 |  1.31% | 0.9727 |
| 08-cssvars-calc              | OK            |  1 |  1 |  +0 | 1.0000  |  1/1  (0.00) | 0.9393 | 0.9393 |  1.43% | 0.9798 |
| 09-justify-hyphens           | OK            |  1 |  1 |  +0 | 0.9973  |  1/1  (1.00) | 0.4708 | 0.4708 | 11.49% | 0.8227 |
| 10-rtl-bidi                  | OK            |  1 |  1 |  +0 | 0.7337  |  1/1  (1.00) | 0.9671 | 0.9671 |  0.63% | 0.9003 |
| 11-cjk-emoji                 | OK            |  1 |  1 |  +0 | 0.8496  |  1/1  (1.00) | 0.9427 | 0.9427 |  1.39% | 0.9308 |
| 12-visual-effects            | OK            |  1 |  1 |  +0 | 0.5750  |  1/1  (1.00) | 0.9352 | 0.9352 |  3.08% | 0.8367 |
| 13-tall-image-objectfit      | OK            |  2 |  3 |  +1 | 1.0000  |  1/1  (1.00) | 0.9543 | 0.9488 |  3.84% | 0.8181 |
| 14-print-media-coloradjust   | OK            |  1 |  1 |  +0 | 0.9058  |  1/1  (1.00) | 0.9215 | 0.9215 |  5.41% | 0.9424 |
| 15-positioned-abs-fixed      | OK            |  2 |  2 |  +0 | 0.8765  |  1/1  (1.00) | 0.9798 | 0.9677 |  0.54% | 0.9521 |
| 16-malformed-nowrap          | OK            |  1 |  1 |  +0 | 0.9529  |  1/1  (0.00) | 0.9335 | 0.9335 |  1.55% | 0.9621 |


**Aggregate (16 OK / 0 errors)**: mean_text_sim=0.9062  mean_ssim=0.8328  min_ssim_overall=0.4708  mean_px_diff=3.88%  mean_score=0.8609  total_|Δpages|=4

        ## Legend

        - **status**: OK | ORACLE_CRASH | ORACLE_ERROR | NEW_CRASH | NEW_ERROR | METRICS_ERROR
        - **p_ref / p_new**: page count from oracle / new engine
        - **Δp**: page count difference (new − ref)
        - **text_sim**: SequenceMatcher ratio on whitespace-normalised extracted text (0=none, 1=identical)
        - **outline ref/new**: TOC entry count; ratio = intersection / max
        - **mean_ssim / min_ssim**: structural similarity over matched pages (1=identical)
        - **px_diff%**: mean absolute pixel difference / 255 × 100 over matched pages
        - **score**: (text_sim + mean_ssim + page_count_closeness) / 3
