        # Fidelity Measurement: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
        Corpus: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/tests/compat/legacy` (3 files)
        DPI for visual comparison: 100

        ## Metrics

        | Document                     | status        | p_ref | p_new |  Δp | text_sim | body_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |
|:-----------------------------|:--------------|------:|------:|----:|---------:|---------:|:----------------|----------:|---------:|---------:|-------:|
| article                      | OK            |  4 |  5 |  +1 | 1.0000  | 0.9875  |  4/4  (0.75) | 0.5111 | 0.4917 |  9.24% | 0.7537 |
| invoice                      | OK            |  1 |  2 |  +1 | 1.0000  | 0.9823  |  1/1  (1.00) | 0.6136 | 0.6136 |  7.97% | 0.5379 |
| report                       | OK            |  4 |  6 |  +2 | 0.9875  | 0.9646  | 13/13 (1.00) | 0.5183 | 0.4596 |  9.88% | 0.6686 |


**Aggregate (3 OK / 0 errors)**: mean_text_sim=0.9958  mean_body_text_sim=0.9781  min_outline_ratio=0.75  mean_ssim=0.5477  min_ssim_overall=0.4596  mean_px_diff=9.029%  mean_score=0.6534  total_|Δpages|=4  max_|Δpages|=2

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
