// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use crate::error::Result;

/// Opaque handle to a loaded page, owned by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHandle(pub u64);

#[derive(Debug, Clone)]
pub enum Source {
    Url(String),
    Html(String),
    Stdin,
}

#[derive(Debug, Clone, Default)]
pub struct LoadSettings {
    pub cookies: Vec<(String, String)>,
    pub custom_headers: Vec<(String, String)>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub proxy: Option<String>,
    pub no_check_certificate: bool,
    pub enable_javascript: bool,
    pub allow_local_file_access: bool,
}

#[derive(Debug, Clone)]
pub struct ReadyPolicy {
    pub javascript_delay_ms: u64,
    pub window_status: Option<String>,
}
impl Default for ReadyPolicy {
    fn default() -> Self { Self { javascript_delay_ms: 0, window_status: None } }
}

#[derive(Debug, Clone, Copy)]
pub enum Orientation { Portrait, Landscape }

#[derive(Debug, Clone)]
pub struct PageGeometry {
    pub width_mm: f64,
    pub height_mm: f64,
    pub margin_top_mm: f64,
    pub margin_bottom_mm: f64,
    pub margin_left_mm: f64,
    pub margin_right_mm: f64,
    pub orientation: Orientation,
    pub scale: f64,
    pub print_background: bool,
    pub prefer_css_page_size: bool,
    pub generate_document_outline: bool,
}
impl Default for PageGeometry {
    fn default() -> Self {
        Self {
            width_mm: 210.0, height_mm: 297.0, // A4
            margin_top_mm: 10.0, margin_bottom_mm: 10.0,
            margin_left_mm: 10.0, margin_right_mm: 10.0,
            orientation: Orientation::Portrait,
            scale: 1.0, print_background: true, prefer_css_page_size: true,
            generate_document_outline: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat { Png, Jpeg }

#[derive(Debug, Clone)]
pub struct SnapshotOpts {
    pub format: ImageFormat,
    pub crop: Option<(u32, u32, u32, u32)>, // x,y,w,h
    pub scale: f64,
    pub quality: u8,
}
impl Default for SnapshotOpts {
    fn default() -> Self { Self { format: ImageFormat::Png, crop: None, scale: 1.0, quality: 94 } }
}

#[derive(Debug, Clone)]
pub struct RawImage { pub bytes: Vec<u8>, pub format: ImageFormat }

#[derive(Debug, Clone)]
pub struct PageInfo { pub title: String, pub final_url: String, pub content_height_px: f64 }

/// The seam. Blocking from the core's view; backends hide the async engine.
pub trait Renderer {
    fn open(&mut self, src: &Source, load: &LoadSettings) -> Result<PageHandle>;
    fn wait_ready(&mut self, page: PageHandle, ready: &ReadyPolicy) -> Result<()>;
    fn eval_json(&mut self, page: PageHandle, script: &str) -> Result<serde_json::Value>;
    fn print_pdf(&mut self, page: PageHandle, geom: &PageGeometry) -> Result<Vec<u8>>;
    fn snapshot(&mut self, page: PageHandle, opts: &SnapshotOpts) -> Result<RawImage>;
    fn page_info(&self, page: PageHandle) -> Result<PageInfo>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtos_have_sensible_defaults() {
        let g = PageGeometry::default();
        assert!(g.width_mm > 0.0 && g.height_mm > 0.0);
        assert!(matches!(SnapshotOpts::default().format, ImageFormat::Png));
        let r = ReadyPolicy::default();
        assert_eq!(r.javascript_delay_ms, 0);
    }
}
