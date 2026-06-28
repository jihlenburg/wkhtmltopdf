// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Typed settings structs for the wkhtmltopdf settings registry.
//
// All length values are stored in **millimetres (mm)** internally.  The
// `parse_length_mm` function converts the wire representation
// ("10mm", "2.5cm", "1in", "96px", "12pt", bare number → mm) at parse time so
// downstream code never needs to carry units.

use crate::assembly::{AssembleOpts, CellText};
use crate::error::{Result, WkError};
use crate::policy::ResourcePolicy;
use crate::render::{Orientation, PageGeometry, Source};

// ---------------------------------------------------------------------------
// Named page sizes
// ---------------------------------------------------------------------------
/// ISO A/B series + common North-American sizes that wkhtmltopdf accepts.
///
/// All dimensions below are in **portrait** orientation (width × height in mm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NamedPageSize {
    A0,
    A1,
    A2,
    A3,
    #[default]
    A4,
    A5,
    A6,
    A7,
    A8,
    A9,
    B0,
    B1,
    B2,
    B3,
    B4,
    B5,
    B6,
    B7,
    B8,
    B9,
    B10,
    C5E,
    Comm10E,
    DLE,
    Executive,
    Folio,
    Ledger,
    Legal,
    Letter,
    Tabloid,
}

impl NamedPageSize {
    /// Portrait (width_mm, height_mm).
    pub fn dimensions_mm(self) -> (f64, f64) {
        match self {
            NamedPageSize::A0 => (841.0, 1189.0),
            NamedPageSize::A1 => (594.0, 841.0),
            NamedPageSize::A2 => (420.0, 594.0),
            NamedPageSize::A3 => (297.0, 420.0),
            NamedPageSize::A4 => (210.0, 297.0),
            NamedPageSize::A5 => (148.0, 210.0),
            NamedPageSize::A6 => (105.0, 148.0),
            NamedPageSize::A7 => (74.0, 105.0),
            NamedPageSize::A8 => (52.0, 74.0),
            NamedPageSize::A9 => (37.0, 52.0),
            NamedPageSize::B0 => (1000.0, 1414.0),
            NamedPageSize::B1 => (707.0, 1000.0),
            NamedPageSize::B2 => (500.0, 707.0),
            NamedPageSize::B3 => (353.0, 500.0),
            NamedPageSize::B4 => (250.0, 353.0),
            NamedPageSize::B5 => (176.0, 250.0),
            NamedPageSize::B6 => (125.0, 176.0),
            NamedPageSize::B7 => (88.0, 125.0),
            NamedPageSize::B8 => (62.0, 88.0),
            NamedPageSize::B9 => (44.0, 62.0),
            NamedPageSize::B10 => (31.0, 44.0),
            NamedPageSize::C5E => (163.0, 229.0),
            NamedPageSize::Comm10E => (105.0, 241.0),
            NamedPageSize::DLE => (110.0, 220.0),
            NamedPageSize::Executive => (184.0, 267.0),
            NamedPageSize::Folio => (210.0, 330.0),
            NamedPageSize::Ledger => (432.0, 279.0),
            NamedPageSize::Legal => (216.0, 356.0),
            NamedPageSize::Letter => (216.0, 279.0),
            NamedPageSize::Tabloid => (279.0, 432.0),
        }
    }

    /// Parse a case-insensitive size name as used by wkhtmltopdf.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "A0" => Some(NamedPageSize::A0),
            "A1" => Some(NamedPageSize::A1),
            "A2" => Some(NamedPageSize::A2),
            "A3" => Some(NamedPageSize::A3),
            "A4" => Some(NamedPageSize::A4),
            "A5" => Some(NamedPageSize::A5),
            "A6" => Some(NamedPageSize::A6),
            "A7" => Some(NamedPageSize::A7),
            "A8" => Some(NamedPageSize::A8),
            "A9" => Some(NamedPageSize::A9),
            "B0" => Some(NamedPageSize::B0),
            "B1" => Some(NamedPageSize::B1),
            "B2" => Some(NamedPageSize::B2),
            "B3" => Some(NamedPageSize::B3),
            "B4" => Some(NamedPageSize::B4),
            "B5" => Some(NamedPageSize::B5),
            "B6" => Some(NamedPageSize::B6),
            "B7" => Some(NamedPageSize::B7),
            "B8" => Some(NamedPageSize::B8),
            "B9" => Some(NamedPageSize::B9),
            "B10" => Some(NamedPageSize::B10),
            "C5E" => Some(NamedPageSize::C5E),
            "COMM10E" => Some(NamedPageSize::Comm10E),
            "DLE" => Some(NamedPageSize::DLE),
            "EXECUTIVE" => Some(NamedPageSize::Executive),
            "FOLIO" => Some(NamedPageSize::Folio),
            "LEDGER" => Some(NamedPageSize::Ledger),
            "LEGAL" => Some(NamedPageSize::Legal),
            "LETTER" => Some(NamedPageSize::Letter),
            "TABLOID" => Some(NamedPageSize::Tabloid),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Color mode
// ---------------------------------------------------------------------------
/// Matches `QPrinter::ColorMode` values accepted by wkhtmltopdf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Color,
    Grayscale,
}

// ---------------------------------------------------------------------------
// Header / footer settings (per row)
// ---------------------------------------------------------------------------
/// Three-cell header or footer row with font metadata.
///
/// Matches the upstream `HeaderFooter` struct fields exposed via the dotted
/// settings API (`header.left`, `header.fontSize`, etc.).
#[derive(Debug, Clone)]
pub struct HeaderFooterSettings {
    pub left: String,
    pub center: String,
    pub right: String,
    /// Point size of the text (default 12, matching upstream).
    pub font_size: f64,
    /// Whether to draw a separator line between the header/footer and the page
    /// body.
    pub line: bool,
}

impl Default for HeaderFooterSettings {
    fn default() -> Self {
        Self {
            left: String::new(),
            center: String::new(),
            right: String::new(),
            font_size: 12.0,
            line: false,
        }
    }
}

impl HeaderFooterSettings {
    /// Returns `Some(CellText)` when at least one cell is non-empty,
    /// `None` when all cells are empty (no header/footer stamp needed).
    pub fn to_cell_text(&self) -> Option<CellText> {
        if self.left.is_empty() && self.center.is_empty() && self.right.is_empty() {
            None
        } else {
            Some(CellText {
                left: self.left.clone(),
                center: self.center.clone(),
                right: self.right.clone(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// GlobalSettings
// ---------------------------------------------------------------------------
/// Document-level settings corresponding to wkhtmltopdf's `PdfGlobal` struct
/// plus selected per-object settings promoted to global scope for convenience.
///
/// All length values stored as **mm**.
#[derive(Debug, Clone)]
pub struct GlobalSettings {
    // --- Size ----------------------------------------------------------
    /// Named page size (default A4).  Overridden by explicit width/height.
    pub page_size: NamedPageSize,
    /// Explicit page width in mm.  When `Some`, overrides `page_size` width.
    pub page_width_mm: Option<f64>,
    /// Explicit page height in mm.  When `Some`, overrides `page_size` height.
    pub page_height_mm: Option<f64>,

    // --- Margins (mm) --------------------------------------------------
    pub margin_top_mm: f64,
    pub margin_bottom_mm: f64,
    pub margin_left_mm: f64,
    pub margin_right_mm: f64,

    // --- Layout --------------------------------------------------------
    pub orientation: Orientation,
    /// Screen resolution in DPI (default 96).
    pub dpi: u32,
    /// Image DPI for printed raster images (default 600, from `imageDPI`).
    pub image_dpi: u32,
    /// JPEG quality 0-100 (default 94, from `imageQuality`).
    pub image_quality: u8,
    pub color_mode: ColorMode,
    /// Global zoom / scale factor (default 1.0).
    pub zoom: f64,

    // --- Web rendering -------------------------------------------------
    pub print_media_type: bool,
    pub enable_javascript: bool,
    pub print_background: bool,

    // --- Load ----------------------------------------------------------
    /// How long to wait after page load before printing (ms, default 200).
    pub javascript_delay_ms: u64,
    /// HTTP/SOCKS proxy URL, e.g. `"http://proxy:8080"`.
    pub proxy: Option<String>,
    /// When `true`, TLS certificate errors are ignored.
    pub no_check_certificate: bool,
    /// When `true`, `file://` URLs are allowed (default matches wkhtmltopdf's
    /// permissive behaviour).
    pub allow_local_file_access: bool,
    /// HTTP Basic-auth username (empty = no auth).
    pub username: String,
    /// HTTP Basic-auth password (empty = no auth).
    pub password: String,
    /// Cookies to send with page requests: list of (name, value) pairs.
    pub cookies: Vec<(String, String)>,
    /// Extra HTTP request headers: list of (name, value) pairs.
    pub custom_headers: Vec<(String, String)>,

    // --- Header / Footer -----------------------------------------------
    pub header: HeaderFooterSettings,
    pub footer: HeaderFooterSettings,

    // --- Document structure --------------------------------------------
    /// Prepend a Table of Contents page (default false).
    pub produce_toc: bool,
    /// Produce interactive AcroForm text fields from HTML inputs (default false,
    /// from `produceForms` / `--enable-forms`).
    pub produce_forms: bool,
    /// Embed a PDF outline / bookmark tree (default true, from `outline`).
    pub produce_outline: bool,
    /// Maximum depth of the generated outline (default 4, from `outlineDepth`).
    pub outline_depth: u32,
    /// Document title substituted for `[title]` in header/footer templates.
    pub document_title: String,
    /// Output file path (empty string = stdout, from `out`).
    pub output_path: String,
    /// Cover page URL. `None` = no cover.
    pub cover: Option<String>,

    // --- TOC XSL / custom stylesheet -----------------------------------
    /// Path to a custom XSLT stylesheet for TOC rendering (`--xsl-style-sheet`).
    /// `None` = use the built-in default TOC renderer.
    pub toc_xsl: Option<String>,
    /// Settings for the built-in default TOC renderer (caption, dotted lines,
    /// indentation, font scale).  Ignored when `toc_xsl` is `Some`.
    pub toc_settings: crate::tocxsl::TocXslSettings,

    // --- Security policy -----------------------------------------------
    /// Activate the hardened `--safe` profile.  When `true`, `to_load_settings`
    /// builds a [`ResourcePolicy`] from [`ResourcePolicy::safe_profile()`] with
    /// any `--allow` paths applied on top.
    pub safe_mode: bool,
    /// Paths that are always permitted even when `allow_local_file_access` is
    /// `false` or `--safe` is active.  Each entry is matched as a path *prefix*
    /// against the decoded path component of the URL.
    pub allowed_paths: Vec<String>,
    /// Block navigational external hyperlinks (`<a href="…">`).  Does NOT
    /// affect subresource loading.
    pub block_external_links: bool,
    /// Block in-page `#anchor` links.
    pub block_internal_links: bool,

    // --- Internal ------------------------------------------------------
    /// Warnings accumulated for recognised-but-unimplemented settings.
    pub warnings: Vec<String>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            page_size: NamedPageSize::A4,
            page_width_mm: None,
            page_height_mm: None,
            margin_top_mm: 10.0,
            margin_bottom_mm: 10.0,
            margin_left_mm: 10.0,
            margin_right_mm: 10.0,
            orientation: Orientation::Portrait,
            dpi: 96,
            image_dpi: 600,
            image_quality: 94,
            color_mode: ColorMode::Color,
            zoom: 1.0,
            print_media_type: false,
            enable_javascript: true,
            print_background: true,
            javascript_delay_ms: 200,
            proxy: None,
            no_check_certificate: false,
            allow_local_file_access: true,
            username: String::new(),
            password: String::new(),
            cookies: Vec::new(),
            custom_headers: Vec::new(),
            header: HeaderFooterSettings::default(),
            footer: HeaderFooterSettings::default(),
            produce_toc: false,
            produce_forms: false,
            produce_outline: true,
            outline_depth: 4,
            document_title: String::new(),
            output_path: String::new(),
            cover: None,
            safe_mode: false,
            allowed_paths: Vec::new(),
            block_external_links: false,
            block_internal_links: false,
            warnings: Vec::new(),
            toc_xsl: None,
            toc_settings: crate::tocxsl::TocXslSettings::default(),
        }
    }
}

impl GlobalSettings {
    /// Drain and return any accumulated warnings, clearing the internal list.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Map the page-size, margin, and orientation fields to a [`PageGeometry`].
    ///
    /// # Orientation swap
    /// When `orientation` is `Landscape` the portrait width and height are
    /// swapped so that `width_mm > height_mm` in the returned geometry.
    pub fn to_geometry(&self) -> PageGeometry {
        let (named_w, named_h) = self.page_size.dimensions_mm();
        let portrait_w = self.page_width_mm.unwrap_or(named_w);
        let portrait_h = self.page_height_mm.unwrap_or(named_h);
        let (width_mm, height_mm) = match self.orientation {
            Orientation::Portrait => (portrait_w, portrait_h),
            Orientation::Landscape => (portrait_h, portrait_w),
        };
        PageGeometry {
            width_mm,
            height_mm,
            margin_top_mm: self.margin_top_mm,
            margin_bottom_mm: self.margin_bottom_mm,
            margin_left_mm: self.margin_left_mm,
            margin_right_mm: self.margin_right_mm,
            orientation: self.orientation,
            scale: self.zoom,
            print_background: self.print_background,
            prefer_css_page_size: false,
            generate_document_outline: self.produce_outline,
        }
    }

    /// Map the header/footer/toc/cover fields to an [`AssembleOpts`].
    ///
    /// `header_footer_font_size` in the returned opts is taken from the header
    /// font size when a header is configured, otherwise from the footer font
    /// size, falling back to the `AssembleOpts` default of 9.0.
    pub fn to_assemble_opts(&self) -> AssembleOpts {
        let header = self.header.to_cell_text();
        let footer = self.footer.to_cell_text();
        let font_size = match (&header, &footer) {
            (Some(_), _) => self.header.font_size,
            (None, Some(_)) => self.footer.font_size,
            (None, None) => 9.0,
        };
        let cover = self.cover.as_ref().map(|url| Source::Url(url.clone()));
        AssembleOpts {
            number: false,
            with_toc: self.produce_toc,
            produce_forms: self.produce_forms,
            header,
            footer,
            header_footer_font_size: font_size,
            doc_title: self.document_title.clone(),
            cover,
            load: self.to_load_settings(),
            toc_xsl: self.toc_xsl.clone(),
            toc_settings: self.toc_settings.clone(),
        }
    }

    /// Build a [`crate::render::LoadSettings`] from the global networking and
    /// security-policy fields.
    ///
    /// When `safe_mode` is `true` the policy starts from
    /// [`ResourcePolicy::safe_profile()`] (local-file denied, private-IP
    /// blocked) and then layered with any `allowed_paths` and link flags.
    /// When `safe_mode` is `false` the policy respects `allow_local_file_access`
    /// (default `true` — permissive, matching historic wkhtmltopdf behaviour).
    pub fn to_load_settings(&self) -> crate::render::LoadSettings {
        let policy = if self.safe_mode {
            ResourcePolicy {
                allowed_paths: self.allowed_paths.clone(),
                allow_external_links: !self.block_external_links,
                allow_internal_links: !self.block_internal_links,
                ..ResourcePolicy::safe_profile()
            }
        } else {
            ResourcePolicy {
                allow_local_file: self.allow_local_file_access,
                allowed_paths: self.allowed_paths.clone(),
                block_private_ips: false,
                allow_external_links: !self.block_external_links,
                allow_internal_links: !self.block_internal_links,
            }
        };

        crate::render::LoadSettings {
            cookies: self.cookies.clone(),
            custom_headers: self.custom_headers.clone(),
            username: if self.username.is_empty() {
                None
            } else {
                Some(self.username.clone())
            },
            password: if self.password.is_empty() {
                None
            } else {
                Some(self.password.clone())
            },
            proxy: self.proxy.clone(),
            no_check_certificate: self.no_check_certificate,
            enable_javascript: self.enable_javascript,
            allow_local_file_access: self.allow_local_file_access,
            compat_ua_css: None,
            policy,
            device_metrics: None,
            load_images: true,
        }
    }
}

// ---------------------------------------------------------------------------
// PdfObjectSettings
// ---------------------------------------------------------------------------
/// Per-object settings corresponding to wkhtmltopdf's `PdfObject` struct.
///
/// One `PdfObjectSettings` is created per HTML page / URL added to the
/// converter.  All length values stored as **mm**.
#[derive(Debug, Clone)]
pub struct PdfObjectSettings {
    /// URL or file path to load.  Corresponds to `page` in the C API.
    pub page: String,

    // --- Web ---
    pub print_media_type: bool,
    pub enable_javascript: bool,
    pub print_background: bool,

    // --- Load ---
    pub javascript_delay_ms: u64,
    pub zoom: f64,
    pub username: String,
    pub password: String,

    // --- Header / Footer ---
    pub header: HeaderFooterSettings,
    pub footer: HeaderFooterSettings,

    // --- Structural flags ---
    pub include_in_outline: bool,
    pub pages_count: bool,
    pub is_table_of_content: bool,
    /// Whether this object's form fields should become interactive AcroForm
    /// fields.  Mirrors the upstream `produceForms` per-object setting.
    /// The assembly currently uses `AssembleOpts.produce_forms` (the global
    /// flag); this per-object value is stored for API compatibility.
    pub produce_forms: bool,

    // --- Internal ---
    pub warnings: Vec<String>,
}

impl Default for PdfObjectSettings {
    fn default() -> Self {
        Self {
            page: String::new(),
            print_media_type: false,
            enable_javascript: true,
            print_background: true,
            javascript_delay_ms: 200,
            zoom: 1.0,
            username: String::new(),
            password: String::new(),
            header: HeaderFooterSettings::default(),
            footer: HeaderFooterSettings::default(),
            include_in_outline: true,
            pages_count: true,
            is_table_of_content: false,
            produce_forms: false,
            warnings: Vec::new(),
        }
    }
}

impl PdfObjectSettings {
    /// Drain and return any accumulated warnings, clearing the internal list.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

// ---------------------------------------------------------------------------
// ImageGlobalSettings
// ---------------------------------------------------------------------------
/// Settings for the wkhtmltoimage converter, matching upstream `ImageGlobal`.
///
/// Naming convention for the C-ABI registry mirrors the upstream reflection
/// system (`imagesettings.cc`) and is documented on each field.
#[derive(Debug, Clone)]
pub struct ImageGlobalSettings {
    // --- Input / Output ---------------------------------------------------
    /// Input URL or file path (`"in"` setting).  `None` = not set.
    pub in_path: Option<String>,
    /// Output file path (`"out"` setting).  `None` = stdout.
    pub out: Option<String>,

    // --- Format -----------------------------------------------------------
    /// Output format: `"png"`, `"jpeg"`, or `"jpg"` (`"fmt"` setting).
    pub fmt: String,
    /// JPEG quality 0–100 (`"quality"` setting).  Ignored for PNG.
    pub quality: u8,

    // --- Viewport / layout ------------------------------------------------
    /// Screen width in pixels for the Chromium viewport (`"screenWidth"`).
    pub screen_width: Option<u32>,
    /// Hint for page height in pixels (`"screenHeight"`).  Informational only.
    pub screen_height: Option<u32>,
    /// When `true` (default), allow the viewport to be wider than
    /// `screen_width` to prevent breaking content (`"smartWidth"`).
    pub smart_width: bool,
    /// Zoom / scale factor (`"zoom"` or `"load.zoomFactor"`).
    pub zoom: f64,

    // --- Crop -------------------------------------------------------------
    /// Pixel crop X offset (`"crop.left"`).
    pub crop_x: Option<u32>,
    /// Pixel crop Y offset (`"crop.top"`).
    pub crop_y: Option<u32>,
    /// Pixel crop width (`"crop.width"`).
    pub crop_w: Option<u32>,
    /// Pixel crop height (`"crop.height"`).
    pub crop_h: Option<u32>,

    // --- PNG transparency -------------------------------------------------
    /// Keep alpha channel in PNG output (`"transparent"`).
    pub transparent: bool,

    // --- Web rendering ---------------------------------------------------
    pub enable_javascript: bool, // "web.enableJavascript"
    pub print_media_type: bool,  // "web.printMediaType"
    pub print_background: bool,  // "web.background"
    /// When `false`, all image resources are blocked (`"web.loadImages"` /
    /// `--no-images`).  Default `true` (permissive).
    pub load_images: bool, // "web.loadImages"

    // --- Load settings ---------------------------------------------------
    /// JavaScript delay in milliseconds (`"load.jsdelay"`).
    pub javascript_delay_ms: u64,
    /// HTTP/SOCKS proxy URL (`"load.proxy"`).
    pub proxy: Option<String>,
    /// Ignore TLS certificate errors (`"load.noCheckCertificate"`).
    pub no_check_certificate: bool,
    /// Allow `file://` URLs (`"load.blockLocalFileAccess"` inverted).
    pub allow_local_file_access: bool,
    /// HTTP Basic-auth username (`"load.username"`).
    pub username: String,
    /// HTTP Basic-auth password (`"load.password"`).
    pub password: String,
    /// Cookies to inject (`"load.cookies"`).
    pub cookies: Vec<(String, String)>,
    /// Extra HTTP request headers (`"load.customHeaders"`).
    pub custom_headers: Vec<(String, String)>,

    // --- Security policy -------------------------------------------------
    /// Activate the hardened `--safe` profile (`"load.safe"`).
    pub safe_mode: bool,
    /// Paths always permitted even in safe mode (`"load.allowedPath"`).
    pub allowed_paths: Vec<String>,
    /// Block external `<a>` links (`"load.disableExternalLinks"`).
    pub block_external_links: bool,
    /// Block same-page `#anchor` links (`"load.disableInternalLinks"`).
    pub block_internal_links: bool,

    // --- Internal --------------------------------------------------------
    /// Warnings accumulated for recognised-but-unimplemented settings.
    pub warnings: Vec<String>,
}

impl Default for ImageGlobalSettings {
    fn default() -> Self {
        Self {
            in_path: None,
            out: None,
            fmt: String::from("png"),
            quality: 94,
            screen_width: Some(1024),
            screen_height: None,
            smart_width: true,
            zoom: 1.0,
            crop_x: None,
            crop_y: None,
            crop_w: None,
            crop_h: None,
            transparent: false,
            enable_javascript: true,
            print_media_type: false,
            print_background: true,
            load_images: true,
            javascript_delay_ms: 200,
            proxy: None,
            no_check_certificate: false,
            allow_local_file_access: true,
            username: String::new(),
            password: String::new(),
            cookies: Vec::new(),
            custom_headers: Vec::new(),
            safe_mode: false,
            allowed_paths: Vec::new(),
            block_external_links: false,
            block_internal_links: false,
            warnings: Vec::new(),
        }
    }
}

impl ImageGlobalSettings {
    /// Drain and return any accumulated warnings, clearing the internal list.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Map the format/quality/crop/transparency fields to an [`crate::image::ImageOpts`].
    pub fn to_image_opts(&self) -> crate::image::ImageOpts {
        use crate::render::ImageFormat;
        let format = match self.fmt.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => ImageFormat::Jpeg,
            _ => ImageFormat::Png,
        };
        let crop = match (self.crop_x, self.crop_y, self.crop_w, self.crop_h) {
            (Some(x), Some(y), Some(w), Some(h)) => Some((x, y, w, h)),
            _ => None,
        };
        crate::image::ImageOpts {
            format,
            width: None, // post-capture resize not driven by settings (use screen_width for viewport)
            height: None,
            quality: self.quality,
            transparent: self.transparent,
            crop,
            zoom: self.zoom,
            screen_width: self.screen_width,
        }
    }

    /// Build a [`crate::render::LoadSettings`] from the networking and
    /// security-policy fields.
    pub fn to_load_settings(&self) -> crate::render::LoadSettings {
        use crate::policy::ResourcePolicy;
        use crate::render::DeviceMetrics;
        let policy = if self.safe_mode {
            ResourcePolicy {
                allowed_paths: self.allowed_paths.clone(),
                allow_external_links: !self.block_external_links,
                allow_internal_links: !self.block_internal_links,
                ..ResourcePolicy::safe_profile()
            }
        } else {
            ResourcePolicy {
                allow_local_file: self.allow_local_file_access,
                allowed_paths: self.allowed_paths.clone(),
                block_private_ips: false,
                allow_external_links: !self.block_external_links,
                allow_internal_links: !self.block_internal_links,
            }
        };

        // Populate device_metrics when any viewport/zoom setting is non-trivial.
        let device_metrics =
            if self.zoom != 1.0 || self.screen_width.is_some() || self.screen_height.is_some() {
                Some(DeviceMetrics {
                    width: self.screen_width.unwrap_or(0),
                    height: self.screen_height.unwrap_or(0),
                    device_scale_factor: if self.zoom > 0.0 { self.zoom } else { 1.0 },
                    smart_width: self.smart_width,
                })
            } else {
                None
            };

        crate::render::LoadSettings {
            cookies: self.cookies.clone(),
            custom_headers: self.custom_headers.clone(),
            username: if self.username.is_empty() {
                None
            } else {
                Some(self.username.clone())
            },
            password: if self.password.is_empty() {
                None
            } else {
                Some(self.password.clone())
            },
            proxy: self.proxy.clone(),
            no_check_certificate: self.no_check_certificate,
            enable_javascript: self.enable_javascript,
            allow_local_file_access: self.allow_local_file_access,
            compat_ua_css: None,
            policy,
            device_metrics,
            load_images: self.load_images,
        }
    }
}

// ---------------------------------------------------------------------------
// Length parser
// ---------------------------------------------------------------------------
/// Parse a length string to **millimetres**.
///
/// Accepted forms (case-insensitive unit suffix):
/// | Input | Multiplier → mm |
/// |---|---|
/// | `"10mm"` / `"10millimeter"` | ×1 |
/// | `"2.5cm"` / `"2.5centimeter"` | ×10 |
/// | `"1m"` / `"1meter"` | ×1000 |
/// | `"1in"` / `"1inch"` | ×25.4 |
/// | `"12pt"` / `"12point"` | ×(25.4/72) |
/// | `"1pc"` / `"1pica"` | ×(25.4/6) |
/// | `"96px"` / `"96pixel"` | ×(25.4/96) |
/// | `"10"` (no unit) | treated as mm |
///
/// Returns `Err(WkError::BadArg)` when the numeric part cannot be parsed or
/// the unit is not in the list above.
pub fn parse_length_mm(s: &str) -> Result<f64> {
    let s = s.trim();
    // Split numeric prefix from unit suffix.
    let num_end = s
        .char_indices()
        .position(|(_, c)| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(s.len());
    let num_str = &s[..num_end];
    let unit = s[num_end..].trim().to_ascii_lowercase();

    let value: f64 = num_str
        .parse()
        .map_err(|_| WkError::BadArg(format!("invalid length: {s:?}")))?;

    let mm = match unit.as_str() {
        "" | "mm" | "millimeter" | "millimeters" => value,
        "cm" | "centimeter" | "centimeters" => value * 10.0,
        "m" | "meter" | "meters" => value * 1000.0,
        "in" | "inch" | "inches" => value * 25.4,
        "pt" | "point" | "points" => value * (25.4 / 72.0),
        "pc" | "pica" | "picas" => value * (25.4 / 6.0),
        "px" | "pixel" | "pixels" => value * (25.4 / 96.0),
        other => {
            return Err(WkError::BadArg(format!(
                "unknown length unit {other:?} in {s:?}"
            )));
        }
    };
    Ok(mm)
}

/// Parse a boolean string accepted by wkhtmltopdf.
///
/// Accepted truthy: `"true"`, `"1"`, `"yes"`, `"on"` (case-insensitive).
/// Accepted falsy:  `"false"`, `"0"`, `"no"`, `"off"` (case-insensitive).
pub fn parse_bool(s: &str) -> Result<bool> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(WkError::BadArg(format!("invalid boolean: {s:?}"))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // --- length parser ---

    #[test]
    fn parse_mm() {
        assert!((parse_length_mm("10mm").unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn parse_cm() {
        assert!((parse_length_mm("2.5cm").unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn parse_in() {
        assert!((parse_length_mm("1in").unwrap() - 25.4).abs() < 1e-9);
    }

    #[test]
    fn parse_px_at_96dpi() {
        // 96px × (25.4/96) = 25.4 mm
        assert!((parse_length_mm("96px").unwrap() - 25.4).abs() < 1e-9);
    }

    #[test]
    fn parse_pt() {
        // 72pt × (25.4/72) = 25.4 mm
        assert!((parse_length_mm("72pt").unwrap() - 25.4).abs() < 1e-9);
    }

    #[test]
    fn parse_bare_number_defaults_to_mm() {
        assert!((parse_length_mm("10").unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn parse_length_rejects_bad_number() {
        assert!(parse_length_mm("abcmm").is_err());
    }

    #[test]
    fn parse_length_rejects_unknown_unit() {
        assert!(parse_length_mm("10furlongs").is_err());
    }

    // --- parse_bool ---

    #[test]
    fn parse_bool_true_variants() {
        assert!(parse_bool("true").unwrap());
        assert!(parse_bool("True").unwrap());
        assert!(parse_bool("1").unwrap());
        assert!(parse_bool("yes").unwrap());
        assert!(parse_bool("on").unwrap());
    }

    #[test]
    fn parse_bool_false_variants() {
        assert!(!parse_bool("false").unwrap());
        assert!(!parse_bool("False").unwrap());
        assert!(!parse_bool("0").unwrap());
        assert!(!parse_bool("no").unwrap());
        assert!(!parse_bool("off").unwrap());
    }

    #[test]
    fn parse_bool_rejects_garbage() {
        assert!(parse_bool("maybe").is_err());
    }

    // --- NamedPageSize ---

    #[test]
    fn named_page_size_a4_dimensions() {
        let (w, h) = NamedPageSize::A4.dimensions_mm();
        assert!((w - 210.0).abs() < 1e-9);
        assert!((h - 297.0).abs() < 1e-9);
    }

    #[test]
    fn named_page_size_parse_case_insensitive() {
        assert_eq!(NamedPageSize::parse("a4"), Some(NamedPageSize::A4));
        assert_eq!(NamedPageSize::parse("A4"), Some(NamedPageSize::A4));
        assert_eq!(NamedPageSize::parse("Letter"), Some(NamedPageSize::Letter));
        assert_eq!(NamedPageSize::parse("LETTER"), Some(NamedPageSize::Letter));
        assert_eq!(NamedPageSize::parse("quuxpaper"), None);
    }

    // --- GlobalSettings::to_geometry ---

    #[test]
    fn to_geometry_a4_portrait() {
        let g = GlobalSettings::default();
        let geom = g.to_geometry();
        assert!((geom.width_mm - 210.0).abs() < 1e-9);
        assert!((geom.height_mm - 297.0).abs() < 1e-9);
        assert!(matches!(geom.orientation, Orientation::Portrait));
        assert!(geom.generate_document_outline); // outline=true by default
    }

    #[test]
    fn to_geometry_landscape_swaps_dimensions() {
        let mut g = GlobalSettings::default();
        g.orientation = Orientation::Landscape;
        let geom = g.to_geometry();
        // Portrait A4 is 210×297; landscape is 297×210.
        assert!((geom.width_mm - 297.0).abs() < 1e-9);
        assert!((geom.height_mm - 210.0).abs() < 1e-9);
    }

    #[test]
    fn to_geometry_explicit_size_overrides_named() {
        let mut g = GlobalSettings::default();
        g.page_width_mm = Some(100.0);
        g.page_height_mm = Some(150.0);
        let geom = g.to_geometry();
        assert!((geom.width_mm - 100.0).abs() < 1e-9);
        assert!((geom.height_mm - 150.0).abs() < 1e-9);
    }

    // --- GlobalSettings::to_assemble_opts ---

    #[test]
    fn to_assemble_opts_toc_and_cover() {
        let mut g = GlobalSettings::default();
        g.produce_toc = true;
        g.cover = Some("https://example.com/cover".into());
        let opts = g.to_assemble_opts();
        assert!(opts.with_toc);
        assert!(opts.cover.is_some());
    }

    #[test]
    fn to_assemble_opts_header_font_size() {
        let mut g = GlobalSettings::default();
        g.header.center = "[page]".into();
        g.header.font_size = 14.0;
        let opts = g.to_assemble_opts();
        assert!((opts.header_footer_font_size - 14.0).abs() < 1e-9);
        let h = opts.header.expect("header should be Some");
        assert_eq!(h.center, "[page]");
    }

    #[test]
    fn to_assemble_opts_no_cells_uses_default_font_size() {
        let g = GlobalSettings::default();
        let opts = g.to_assemble_opts();
        assert!(opts.header.is_none());
        assert!(opts.footer.is_none());
        assert!((opts.header_footer_font_size - 9.0).abs() < 1e-9);
    }

    #[test]
    fn image_settings_forward_zoom_and_screen_width_to_device_metrics() {
        let g = ImageGlobalSettings {
            zoom: 2.0,
            screen_width: Some(800),
            smart_width: false,
            ..Default::default()
        };
        let ls = g.to_load_settings();
        let dm = ls
            .device_metrics
            .expect("device_metrics set when zoom/width given");
        assert_eq!(dm.width, 800);
        assert!((dm.device_scale_factor - 2.0).abs() < 1e-9);
        assert!(!dm.smart_width);
    }
}
