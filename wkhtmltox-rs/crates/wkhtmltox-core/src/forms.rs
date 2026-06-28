// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! HTML form-field extraction → interactive PDF AcroForm fields.
//! v1 scope: text inputs and textareas. Checkbox/radio/select are deferred.
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum FormFieldKind {
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    pub name: String,
    pub kind: FormFieldKind,
    /// Document-absolute top Y in CSS px (for source-page derivation).
    pub top: f64,
    /// Bounding rect [x0, y0, x1, y1] in document-absolute CSS px.
    pub rect: [f64; 4],
}

/// JS injected via `Renderer::eval_json`. Mirrors `outline::PROBE_JS`'s
/// coordinate handling: viewport-relative rects shifted by scrollY.
pub const FORM_PROBE_JS: &str = r#"(() => {
  const sy = (typeof window !== 'undefined' && window.scrollY) || 0;
  const sel = 'input[type=text], input:not([type]), textarea';
  const fs = [...document.querySelectorAll(sel)].map(e => {
    const r = e.getBoundingClientRect();
    return {
      name: e.getAttribute('name') || e.id || '',
      kind: 'text',
      top: r.top + sy,
      rect: [r.left, r.top + sy, r.right, r.bottom + sy]
    };
  }).filter(f => f.name && f.rect[2] > f.rect[0] && f.rect[3] > f.rect[1]);
  return { fields: fs };
})()"#;

pub fn parse_form_fields(v: &Value) -> Vec<FormField> {
    v.get("fields")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?;
                    if name.is_empty() {
                        return None;
                    }
                    let rect = f.get("rect")?.as_array()?;
                    if rect.len() != 4 {
                        return None;
                    }
                    let r = |i: usize| rect[i].as_f64();
                    // Filter degenerate rects (width or height ≤ 0).
                    let (x0, y0, x1, y1) = (r(0)?, r(1)?, r(2)?, r(3)?);
                    if x1 <= x0 || y1 <= y0 {
                        return None;
                    }
                    Some(FormField {
                        name: name.to_string(),
                        kind: FormFieldKind::Text,
                        top: f.get("top")?.as_f64()?,
                        rect: [x0, y0, x1, y1],
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_text_fields() {
        let v = serde_json::json!({ "fields": [
            { "name": "email", "kind": "text", "top": 100.0, "rect": [10.0, 100.0, 210.0, 124.0] }
        ]});
        let fs = parse_form_fields(&v);
        assert_eq!(fs.len(), 1);
        assert_eq!(fs[0].name, "email");
        assert_eq!(fs[0].kind, FormFieldKind::Text);
        assert_eq!(fs[0].rect, [10.0, 100.0, 210.0, 124.0]);
    }

    #[test]
    fn skips_unnamed_and_degenerate() {
        let v = serde_json::json!({ "fields": [
            { "name": "", "kind": "text", "top": 0.0, "rect": [0.0, 0.0, 10.0, 10.0] }
        ]});
        assert!(parse_form_fields(&v).is_empty());
    }
}
