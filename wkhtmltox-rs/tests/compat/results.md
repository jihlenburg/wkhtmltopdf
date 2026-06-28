        # Fidelity Measurement: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

        Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
        Corpus: `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/tests/compat/corpus` (6 files)
        DPI for visual comparison: 100

        ## Metrics

        | Document     | p_ref | p_new |  Δp | text_sim | outline ref/new | mean_ssim | min_ssim | px_diff% | score  |
|:-------------|------:|------:|----:|---------:|:----------------|----------:|---------:|---------:|-------:|
| cssbox       |     1 |     1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8007 | 0.8007 |  4.42% | 0.9336 |
| fonts        |     1 |     1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8670 | 0.8670 |  2.24% | 0.9557 |
| headings     |     1 |     2 |  +1 | 1.0000  | 11/11 (1.00) | 0.6094 | 0.6094 |  9.45% | 0.5365 |
| longtext     |     2 |     3 |  +1 | 1.0000  |  1/1  (1.00) | 0.4303 | 0.4079 | 13.20% | 0.6434 |
| table        |     1 |     1 |  +0 | 1.0000  |  1/1  (1.00) | 0.8370 | 0.8370 |  3.72% | 0.9457 |
| text         |     1 |     1 |  +0 | 1.0000  |  1/1  (1.00) | 0.7601 | 0.7601 |  5.50% | 0.9200 |


**Aggregate (6 docs)**: mean_text_sim=1.0  mean_ssim=0.7174  min_ssim_overall=0.4079  mean_px_diff=6.423%  mean_score=0.8225  total_|Δpages|=2

        ## Legend

        - **p_ref / p_new**: page count from oracle / new engine
        - **Δp**: page count difference (new − ref)
        - **text_sim**: SequenceMatcher ratio on whitespace-normalised extracted text (0=none, 1=identical)
        - **outline ref/new**: TOC entry count; ratio = intersection / max
        - **mean_ssim / min_ssim**: structural similarity over matched pages (1=identical)
        - **px_diff%**: mean absolute pixel difference / 255 × 100 over matched pages
        - **score**: (text_sim + mean_ssim + page_count_closeness) / 3
