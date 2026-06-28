// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.

/// JS injected via `Renderer::eval_json` to extract document structure as JSON.
///
/// Returns `{ headings, links }` where:
/// - `headings`: H1–H6 elements with level, text, anchor, page (page always 0 here).
/// - `links`: internal `<a href="#…">` elements with href, internal flag, the
///   document-absolute top Y in CSS px, and a bounding rect `[x0, y0, x1, y1]`
///   also in document-absolute CSS px.
///
/// **Coordinate note:** `getBoundingClientRect()` is viewport-relative; in print
/// mode `window.scrollY` is 0, so the coordinates approximate the page-relative
/// position.  The mapping from CSS px to PDF user-space points is approximate
/// (96→72 dpi, bottom-up y flip) and is documented as such in `assembly.rs`.
pub const PROBE_JS: &str = r#"(() => {
  const hs = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')].map(h => ({
    level: Number(h.tagName.substring(1)),
    text: (h.textContent || '').trim(),
    anchor: h.id || null,
    page: 0
  }));
  const sy = (typeof window !== 'undefined' && window.scrollY) || 0;
  const ls = [...document.querySelectorAll('a[href]')]
    .filter(a => { const h = a.getAttribute('href') || ''; return h.startsWith('#'); })
    .map(a => {
      const r = a.getBoundingClientRect();
      return {
        href: a.getAttribute('href'),
        internal: true,
        top: r.top + sy,
        rect: [r.left, r.top + sy, r.right, r.bottom + sy]
      };
    });
  return { headings: hs, links: ls };
})()"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub anchor: Option<String>,
    pub page: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    pub title: String,
    pub page: u32,
    pub children: Vec<OutlineNode>,
}

pub fn parse_probe(v: &serde_json::Value) -> Vec<Heading> {
    v.get("headings")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    Some(Heading {
                        level: h.get("level")?.as_u64()? as u8,
                        text: h.get("text")?.as_str()?.to_string(),
                        anchor: h
                            .get("anchor")
                            .and_then(|a| a.as_str())
                            .map(|s| s.to_string()),
                        page: h.get("page").and_then(|p| p.as_u64()).unwrap_or(0) as u32,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A single internal `<a href="#…">` link extracted from the probe JSON.
#[derive(Debug, Clone)]
pub struct ProbeLink {
    /// The raw `href` attribute value, e.g. `"#section-1"`.
    pub href: String,
    /// Always `true` for links collected by the probe (href starts with `#`).
    pub internal: bool,
    /// Document-absolute top of the link element in CSS px (approx. in print mode).
    pub top: f64,
    /// `[x0, y0, x1, y1]` bounding rect in document-absolute CSS px.
    pub rect: [f64; 4],
}

/// Parse the `links` array from a probe JSON value.
///
/// Returns an empty `Vec` if the field is missing or malformed.
/// Malformed individual entries are silently skipped.
pub fn parse_probe_links(v: &serde_json::Value) -> Vec<ProbeLink> {
    v.get("links")
        .and_then(|l| l.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let href = item.get("href")?.as_str()?.to_string();
                    let internal = item
                        .get("internal")
                        .and_then(|b| b.as_bool())
                        .unwrap_or_else(|| href.starts_with('#'));
                    let top = item.get("top")?.as_f64()?;
                    let rect_arr = item.get("rect")?.as_array()?;
                    if rect_arr.len() < 4 {
                        return None;
                    }
                    let rect = [
                        rect_arr[0].as_f64()?,
                        rect_arr[1].as_f64()?,
                        rect_arr[2].as_f64()?,
                        rect_arr[3].as_f64()?,
                    ];
                    Some(ProbeLink {
                        href,
                        internal,
                        top,
                        rect,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Nest a flat heading list into a tree by `level` (a deeper heading becomes a
/// child of the nearest preceding shallower one).
pub fn build_outline(headings: &[Heading]) -> Vec<OutlineNode> {
    fn build(
        it: &mut std::iter::Peekable<std::slice::Iter<Heading>>,
        parent_level: u8,
    ) -> Vec<OutlineNode> {
        let mut out = Vec::new();
        while let Some(h) = it.peek() {
            if h.level <= parent_level {
                break;
            }
            let cur = (*h).clone();
            it.next();
            let children = build(it, cur.level);
            out.push(OutlineNode {
                title: cur.text,
                page: cur.page,
                children,
            });
        }
        out
    }
    let mut it = headings.iter().peekable();
    build(&mut it, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn probe() -> serde_json::Value {
        serde_json::json!({"headings":[
            {"level":1,"text":"A","anchor":"a","page":1},
            {"level":2,"text":"A.1","anchor":"a1","page":1},
            {"level":1,"text":"B","anchor":"b","page":3}
        ]})
    }
    #[test]
    fn parses_and_nests() {
        let hs = parse_probe(&probe());
        assert_eq!(hs.len(), 3);
        let tree = build_outline(&hs);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].title, "A");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].page, 1);
        assert_eq!(tree[1].title, "B");
        assert_eq!(tree[1].page, 3);
    }
    #[test]
    fn probe_js_is_present() {
        assert!(PROBE_JS.contains("headings"));
        assert!(
            PROBE_JS.contains("links"),
            "PROBE_JS must also collect links"
        );
    }

    #[test]
    fn parse_probe_links_extracts_internal_link() {
        let v = serde_json::json!({
            "headings": [],
            "links": [
                {"href": "#x", "internal": true, "top": 100.0, "rect": [10.0, 100.0, 200.0, 120.0]}
            ]
        });
        let links = parse_probe_links(&v);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].href, "#x");
        assert!(links[0].internal);
        assert_eq!(links[0].top, 100.0);
        assert_eq!(links[0].rect, [10.0, 100.0, 200.0, 120.0]);
    }

    #[test]
    fn parse_probe_links_empty_when_no_links_field() {
        let v = serde_json::json!({ "headings": [] });
        assert!(parse_probe_links(&v).is_empty());
    }
}
