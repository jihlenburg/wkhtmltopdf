// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use crate::error::Result;
use crate::render::*;

/// A scriptable in-memory Renderer for browser-free tests.
pub struct MockRenderer {
    pub pdf: Vec<u8>,
    pub probe: serde_json::Value,
    pub info: PageInfo,
    next: u64,
}

impl MockRenderer {
    pub fn new() -> Self {
        Self {
            pdf: b"%PDF-1.4\n%mock\n".to_vec(),
            probe: serde_json::json!({ "title": "mock", "headings": [], "anchors": [] }),
            info: PageInfo { title: "mock".into(), final_url: "about:blank".into(), content_height_px: 1000.0 },
            next: 0,
        }
    }
}

impl Default for MockRenderer { fn default() -> Self { Self::new() } }

impl Renderer for MockRenderer {
    fn open(&mut self, _s: &Source, _l: &LoadSettings) -> Result<PageHandle> {
        self.next += 1;
        Ok(PageHandle(self.next))
    }
    fn wait_ready(&mut self, _p: PageHandle, _r: &ReadyPolicy) -> Result<()> { Ok(()) }
    fn eval_json(&mut self, _p: PageHandle, _s: &str) -> Result<serde_json::Value> { Ok(self.probe.clone()) }
    fn print_pdf(&mut self, _p: PageHandle, _g: &PageGeometry) -> Result<Vec<u8>> { Ok(self.pdf.clone()) }
    fn snapshot(&mut self, _p: PageHandle, o: &SnapshotOpts) -> Result<RawImage> {
        Ok(RawImage { bytes: vec![0u8; 8], format: o.format })
    }
    fn page_info(&self, _p: PageHandle) -> Result<PageInfo> { Ok(self.info.clone()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::WkError;
    #[test]
    fn mock_roundtrips() -> Result<()> {
        let mut r = MockRenderer::new();
        let p = r.open(&Source::Html("<p>x</p>".into()), &LoadSettings::default())?;
        r.wait_ready(p, &ReadyPolicy::default())?;
        let pdf = r.print_pdf(p, &PageGeometry::default())?;
        assert!(pdf.starts_with(b"%PDF"));
        let _ : WkError = WkError::Render("ok".into()); // type is reachable
        Ok(())
    }
}
