      # Assembly Comparison: wkhtmltopdf 0.12.6 vs wkhtmltox-render-chromium

      Oracle: `/Users/jihlenburg/.local/wkhtmltox/bin/wkhtmltopdf`
      Inputs: `text.html` + `headings.html`
      New engine: `cargo run --example assemble -p wkhtmltox-render-chromium`

      ## Page Count

      | Side    | Pages |
      |:--------|------:|
      | Oracle  | 2 |
      | New     | 3 |
      | Δ       | +1 |
      | Within ±1 tolerance | Yes |

      ## Outline (Bookmark) Comparison

      | Metric                      | Value |
      |:----------------------------|------:|
      | Oracle outline entries      | 12 |
      | New engine outline entries  | 12 |
      | Title overlap (intersection/max) | 1.000 (12/12) |

      ### Oracle bookmark titles
- Sample Document Heading
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

      ### New-engine bookmark titles
- Sample Document Heading
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

      ### Titles only in oracle
(none)

      ### Titles only in new engine
(none)

      ## Text Similarity

      | Metric     | Value  |
      |:-----------|-------:|
      | text_sim   | 0.9974 |

      ## Notes

      - Outline page numbers are NOT compared (M2a: headings point to object-first-page only;
        per-heading exact destinations deferred to Milestone 2b).
      - Page count tolerance ±1 accepted because Chrome and wkhtmltopdf paginate
        identically-sized content with minor differences.
      - Title overlap is the structural fidelity signal: are the same section titles
        present in both outlines?
