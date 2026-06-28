// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
use crate::error::Result;
use crate::policy::ResourcePolicy;

/// Opaque handle to a loaded page, owned by the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHandle(pub u64);

#[derive(Debug, Clone)]
pub enum Source {
    Url(String),
    Html(String),
    Stdin,
}

/// Viewport/scale overrides applied via CDP `Emulation.setDeviceMetricsOverride`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceMetrics {
    pub width: u32,               // 0 = let Chrome choose
    pub height: u32,              // 0 = let Chrome choose
    pub device_scale_factor: f64, // 1.0 = no zoom
    pub smart_width: bool,        // expand width to content after load
}

#[derive(Debug, Clone)]
pub struct LoadSettings {
    pub cookies: Vec<(String, String)>,
    pub custom_headers: Vec<(String, String)>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub proxy: Option<String>,
    pub no_check_certificate: bool,
    /// When `true`, JavaScript execution is enabled for this page load.
    /// Note that the legacy wkhtmltopdf binary defaults JavaScript **on** —
    /// callers must opt in explicitly to match that behaviour.
    pub enable_javascript: bool,
    pub allow_local_file_access: bool,
    /// When set, the renderer injects this CSS as a UA-reset `<style>` before
    /// printing, to nudge output toward the wkhtmltopdf/Qt4-WebKit baseline.
    pub compat_ua_css: Option<String>,
    /// Resource policy governing which URLs the renderer may load.
    ///
    /// Enforced via CDP `Fetch` interception during `open()`.  The **default**
    /// is permissive (matching historic wkhtmltopdf behaviour).  Use
    /// [`ResourcePolicy::safe_profile()`] or `--safe` to harden.
    pub policy: ResourcePolicy,
    /// Viewport/scale overrides forwarded to CDP `Emulation.setDeviceMetricsOverride`.
    /// `None` = let Chrome use its default viewport.
    pub device_metrics: Option<DeviceMetrics>,
    /// When `false`, all Image-type resources are blocked via the Fetch pump.
    /// Default `true` (permissive, matching historic wkhtmltopdf behaviour).
    pub load_images: bool,
}

impl Default for LoadSettings {
    fn default() -> Self {
        Self {
            cookies: Vec::new(),
            custom_headers: Vec::new(),
            username: None,
            password: None,
            proxy: None,
            no_check_certificate: false,
            enable_javascript: false,
            allow_local_file_access: false,
            compat_ua_css: None,
            policy: ResourcePolicy::default(),
            device_metrics: None,
            load_images: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReadyPolicy {
    pub javascript_delay_ms: u64,
    pub window_status: Option<String>,
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
