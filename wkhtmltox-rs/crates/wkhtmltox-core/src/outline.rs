// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.

/// JS injected via `Renderer::eval_json` to extract document structure as JSON.
/// `page` is filled later from engine destinations; here it defaults to 0.
pub const PROBE_JS: &str = r#"(() => {
  const hs = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')].map(h => ({
    level: Number(h.tagName.substring(1)),
    text: (h.textContent || '').trim(),
    anchor: h.id || null,
    page: 0
  }));
  return { headings: hs };
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
                        page: h
                            .get("page")
                            .and_then(|p| p.as_u64())
                            .unwrap_or(0) as u32,
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
    }
}
