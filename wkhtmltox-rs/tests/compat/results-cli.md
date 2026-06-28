      # M3b CLI Comparison: wkhtmltopdf (ours) vs oracle 0.12.6

      Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
      Ours:   `/Users/jihlenburg/src/wkhtmltopdf/wkhtmltox-rs/target/debug/wkhtmltopdf`
      Inputs: `headings.html` + `longtext.html`

      Oracle invocation: `-s A4 --outline toc headings.html longtext.html out.pdf`
      Ours   invocation: `-s A4 --toc headings.html longtext.html out.pdf`

      Note: oracle uses the `toc` positional subcommand; ours uses `--toc` flag.

      ## Structural Comparison

      ### Page Count

      | Side    | Pages |
      |:--------|------:|
      | Oracle  | 4 |
      | Ours    | 6 |
      | Δ       | +2 |
      | Within ±2 tolerance | Yes |

      ### TOC Page Detection

      | Side    | TOC page present |
      |:--------|:-----------------|
      | Oracle  | True |
      | Ours    | True |

      ### Outline (Bookmark) Comparison

      | Metric                           | Value |
      |:---------------------------------|------:|
      | Oracle outline entries           | 13 |
      | Ours outline entries             | 13 |
      | Title overlap (intersection/max) | 1.000 (13/13) |

      #### Oracle bookmark titles
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

      #### Ours bookmark titles
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

      #### Titles only in oracle
(none)

      #### Titles only in ours
(none)

      ## Exit-Code Comparison

      | Scenario             | oracle rc  | ours rc  | match |
      |:---------------------|:----------:|:--------:|:-----:|
      | (a) success          |          0 |        0 | YES   |
      | (b) unknown flag     |          1 |        1 | YES   |
      | (c) missing input    |          1 |        0 | NO    |

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
