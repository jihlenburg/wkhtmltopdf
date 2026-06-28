      # M2b Assembly Comparison: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

      Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
      Inputs: cover page + `headings.html` + `longtext.html`
      Footer template: `[page]/[topage]`
      New engine: `cargo run --example assemble -- --toc --cover cover.html --footer-center "[page]/[topage]"`

      ## Page Count

      | Side    | Pages |
      |:--------|------:|
      | Oracle  | 5 |
      | New     | 7 |
      | Δ       | +2 |
      | Within ±2 tolerance | Yes |

      > Note: ±2 tolerance accepted because cover + TOC add pages and Chrome/wkhtmltopdf
      > paginate slightly differently.

      ## TOC Page Detection

      | Side    | TOC page present |
      |:--------|:-----------------|
      | Oracle  | True |
      | New     | True |

      ## Outline (Bookmark) Comparison

      | Metric                           | Value |
      |:---------------------------------|------:|
      | Oracle outline entries           | 13 |
      | New engine outline entries       | 13 |
      | Title overlap (intersection/max) | 1.000 (13/13) |

      ### Oracle bookmark titles
- Table of Contents
- Chapter 1: Introduction
- 1.1 Background
- 1.1.1 Historical Context
- 1.1.2 Prior Work
- 1.2 Problem Statement
- 1.2.1 Scope
- 1.2.2 Constraints
- Chapter 2: Methodology
- 2.1 Approach
- 2.1.1 Algorithm
- 2.2 Evaluation
- Long Document — Pagination Drift Test

      ### New-engine bookmark titles
- Table of Contents
- Chapter 1: Introduction
- 1.1 Background
- 1.1.1 Historical Context
- 1.1.2 Prior Work
- 1.2 Problem Statement
- 1.2.1 Scope
- 1.2.2 Constraints
- Chapter 2: Methodology
- 2.1 Approach
- 2.1.1 Algorithm
- 2.2 Evaluation
- Long Document — Pagination Drift Test

      ### Titles only in oracle
(none)

      ### Titles only in new engine
(none)

      ## Text Similarity

      | Metric   | Value  |
      |:---------|-------:|
      | text_sim | 0.1528 |

      ## Notes

      - Oracle uses native `cover` + `toc` subcommands; new engine uses `assemble --toc --cover`.
      - Outline page numbers are structural only; exact pages compared via oracle in M2b Task 1.
      - TOC page detection: searches for "table of contents" or "contents" in page text.
      - Source::Html is now implemented in ChromiumRenderer (temp-file + file:// URL approach).
