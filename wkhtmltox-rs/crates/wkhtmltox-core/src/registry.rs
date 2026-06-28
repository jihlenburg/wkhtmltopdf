// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Settings registry: wkhtmltopdf dotted names → typed fields.
//
// Two public entry points:
//   `set_global(g, name, value)` – set a `GlobalSettings` field by name.
//   `set_object(o, name, value)` – set a `PdfObjectSettings` field by name.
//
// Behaviour matrix:
// | Category | Result |
// |---|---|
// | Implemented name | `Ok(())` + field assigned |
// | Recognised-but-unimplemented name | `Ok(())` + warning appended to settings |
// | Unknown name | `Err(WkError::BadArg(...))` |

use crate::error::{Result, WkError};
use crate::render::Orientation;
use crate::settings::{
    parse_bool, parse_length_mm, ColorMode, GlobalSettings, ImageGlobalSettings, NamedPageSize,
    PdfObjectSettings,
};

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

/// Set a wkhtmltopdf global-settings field on `g` by its canonical dotted name.
///
/// Recognised names that the current engine does not yet implement are silently
/// accepted and a human-readable warning is pushed to `g.warnings`.
///
/// # Errors
/// Returns `Err(WkError::BadArg)` when `name` is not a known wkhtmltopdf
/// setting name, or when `value` cannot be parsed for the target field type.
pub fn set_global(g: &mut GlobalSettings, name: &str, value: &str) -> Result<()> {
    match name {
        // ── size ──────────────────────────────────────────────────────────
        "size.pageSize" => {
            g.page_size = NamedPageSize::parse(value)
                .ok_or_else(|| WkError::BadArg(format!("unknown page size: {value:?}")))?;
        }
        "size.width" => {
            g.page_width_mm = Some(parse_length_mm(value)?);
        }
        "size.height" => {
            g.page_height_mm = Some(parse_length_mm(value)?);
        }

        // ── margins ───────────────────────────────────────────────────────
        "margin.top" => g.margin_top_mm = parse_length_mm(value)?,
        "margin.bottom" => g.margin_bottom_mm = parse_length_mm(value)?,
        "margin.left" => g.margin_left_mm = parse_length_mm(value)?,
        "margin.right" => g.margin_right_mm = parse_length_mm(value)?,

        // ── layout ────────────────────────────────────────────────────────
        "orientation" => {
            let lower = value.trim().to_ascii_lowercase();
            g.orientation = match lower.as_str() {
                "landscape" => Orientation::Landscape,
                "portrait" => Orientation::Portrait,
                _ => {
                    return Err(WkError::BadArg(format!(
                        "unknown orientation {value:?}: expected \"Portrait\" or \"Landscape\""
                    )));
                }
            };
        }
        "dpi" => {
            g.dpi = value
                .trim()
                .parse::<u32>()
                .map_err(|_| WkError::BadArg(format!("invalid dpi: {value:?}")))?;
        }
        "imageDPI" => {
            g.image_dpi = value
                .trim()
                .parse::<u32>()
                .map_err(|_| WkError::BadArg(format!("invalid imageDPI: {value:?}")))?;
        }
        "imageQuality" => {
            g.image_quality = value
                .trim()
                .parse::<u8>()
                .map_err(|_| WkError::BadArg(format!("invalid imageQuality: {value:?}")))?;
        }
        "colorMode" => {
            let lower = value.trim().to_ascii_lowercase();
            g.color_mode = match lower.as_str() {
                "color" => ColorMode::Color,
                "grayscale" => ColorMode::Grayscale,
                _ => {
                    return Err(WkError::BadArg(format!(
                        "unknown colorMode {value:?}: expected \"color\" or \"grayscale\""
                    )));
                }
            };
        }
        "zoom" | "load.zoomFactor" => {
            g.zoom = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid zoom: {value:?}")))?;
        }

        // ── web rendering (forwarded to global scope) ─────────────────────
        "web.printMediaType" => g.print_media_type = parse_bool(value)?,
        "web.enableJavascript" => g.enable_javascript = parse_bool(value)?,
        "web.background" => g.print_background = parse_bool(value)?,

        // ── load (forwarded to global scope) ──────────────────────────────
        "load.jsdelay" => {
            g.javascript_delay_ms = value
                .trim()
                .parse::<u64>()
                .map_err(|_| WkError::BadArg(format!("invalid load.jsdelay: {value:?}")))?;
        }
        "load.proxy" => {
            g.proxy = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "load.blockLocalFileAccess" => {
            g.allow_local_file_access = !parse_bool(value)?;
        }
        "load.username" => g.username = value.to_string(),
        "load.password" => g.password = value.to_string(),

        // ── Security policy ───────────────────────────────────────────────
        "load.safe" => g.safe_mode = parse_bool(value)?,
        // Repeatable: each call appends one allowed path prefix.
        "load.allowedPath" => {
            if !value.is_empty() {
                g.allowed_paths.push(value.to_string());
            }
        }
        "load.disableExternalLinks" => g.block_external_links = parse_bool(value)?,
        "load.disableInternalLinks" => g.block_internal_links = parse_bool(value)?,

        // ── header ────────────────────────────────────────────────────────
        "header.left" => g.header.left = value.to_string(),
        "header.center" => g.header.center = value.to_string(),
        "header.right" => g.header.right = value.to_string(),
        "header.fontSize" => {
            g.header.font_size = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid header.fontSize: {value:?}")))?;
        }
        "header.line" => g.header.line = parse_bool(value)?,

        // ── footer ────────────────────────────────────────────────────────
        "footer.left" => g.footer.left = value.to_string(),
        "footer.center" => g.footer.center = value.to_string(),
        "footer.right" => g.footer.right = value.to_string(),
        "footer.fontSize" => {
            g.footer.font_size = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid footer.fontSize: {value:?}")))?;
        }
        "footer.line" => g.footer.line = parse_bool(value)?,

        // ── document structure ────────────────────────────────────────────
        "toc" => g.produce_toc = parse_bool(value)?,
        "produceForms" => g.produce_forms = parse_bool(value)?,
        "outline" => g.produce_outline = parse_bool(value)?,
        "outlineDepth" => {
            g.outline_depth = value
                .trim()
                .parse::<u32>()
                .map_err(|_| WkError::BadArg(format!("invalid outlineDepth: {value:?}")))?;
        }
        "documentTitle" => g.document_title = value.to_string(),
        "out" => g.output_path = value.to_string(),
        "cover" => {
            g.cover = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }

        // ── TOC XSL / custom stylesheet ───────────────────────────────────
        "tocXsl" => {
            g.toc_xsl = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "toc.captionText" => {
            g.toc_settings.caption_text = value.to_string();
        }
        "toc.useDottedLines" => {
            g.toc_settings.use_dotted_lines = parse_bool(value)?;
        }
        "toc.forwardLinks" => {
            g.toc_settings.forward_links = parse_bool(value)?;
        }
        "toc.backLinks" => {
            g.toc_settings.back_links = parse_bool(value)?;
        }
        "toc.indentation" => {
            g.toc_settings.indentation = value.to_string();
        }
        "toc.fontScale" => {
            g.toc_settings.font_scale = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid toc.fontScale: {value:?}")))?;
        }

        // ── Recognised-but-unimplemented ──────────────────────────────────
        // These names are valid in upstream wkhtmltopdf but are not yet
        // forwarded to the Chromium engine.  We accept them silently and
        // record a warning so callers can audit which settings were dropped.
        "logLevel"
        | "quiet"
        | "useGraphics"
        | "resolveRelativeLinks"
        | "resolution"
        | "pageOffset"
        | "copies"
        | "collate"
        | "dumpOutline"
        | "useCompression"
        | "viewportSize"
        | "load.cookieJar"
        | "web.enableIntelligentShrinking"
        | "web.minimumFontSize"
        | "web.defaultEncoding"
        | "web.userStyleSheet"
        | "web.enablePlugins"
        | "web.loadImages"
        | "header.htmlUrl"
        | "header.fontName"
        | "header.spacing"
        | "footer.htmlUrl"
        | "footer.fontName"
        | "footer.spacing" => {
            g.warnings.push(format!(
                "setting {name:?} is recognised but not yet implemented in this engine; ignored"
            ));
        }

        // ── Unknown ───────────────────────────────────────────────────────
        _ => {
            return Err(WkError::BadArg(format!("unknown setting: {name:?}")));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Object settings
// ---------------------------------------------------------------------------

/// Set a wkhtmltopdf per-object-settings field on `o` by its canonical dotted
/// name.
///
/// Recognised-but-unimplemented names are accepted and a warning is pushed to
/// `o.warnings`.
///
/// # Errors
/// Returns `Err(WkError::BadArg)` when `name` is not a known wkhtmltopdf
/// setting name, or when `value` cannot be parsed for the target field type.
pub fn set_object(o: &mut PdfObjectSettings, name: &str, value: &str) -> Result<()> {
    match name {
        // ── source ────────────────────────────────────────────────────────
        "page" => o.page = value.to_string(),

        // ── web ───────────────────────────────────────────────────────────
        "web.printMediaType" => o.print_media_type = parse_bool(value)?,
        "web.enableJavascript" => o.enable_javascript = parse_bool(value)?,
        "web.background" => o.print_background = parse_bool(value)?,

        // ── load ──────────────────────────────────────────────────────────
        "load.jsdelay" => {
            o.javascript_delay_ms = value
                .trim()
                .parse::<u64>()
                .map_err(|_| WkError::BadArg(format!("invalid load.jsdelay: {value:?}")))?;
        }
        "load.zoomFactor" => {
            o.zoom = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid load.zoomFactor: {value:?}")))?;
        }
        "load.username" => o.username = value.to_string(),
        "load.password" => o.password = value.to_string(),

        // ── header ────────────────────────────────────────────────────────
        "header.left" => o.header.left = value.to_string(),
        "header.center" => o.header.center = value.to_string(),
        "header.right" => o.header.right = value.to_string(),
        "header.fontSize" => {
            o.header.font_size = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid header.fontSize: {value:?}")))?;
        }
        "header.line" => o.header.line = parse_bool(value)?,

        // ── footer ────────────────────────────────────────────────────────
        "footer.left" => o.footer.left = value.to_string(),
        "footer.center" => o.footer.center = value.to_string(),
        "footer.right" => o.footer.right = value.to_string(),
        "footer.fontSize" => {
            o.footer.font_size = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid footer.fontSize: {value:?}")))?;
        }
        "footer.line" => o.footer.line = parse_bool(value)?,

        // ── structural flags ──────────────────────────────────────────────
        "includeInOutline" => o.include_in_outline = parse_bool(value)?,
        "pagesCount" => o.pages_count = parse_bool(value)?,
        "isTableOfContent" => o.is_table_of_content = parse_bool(value)?,
        "produceForms" => o.produce_forms = parse_bool(value)?,

        // ── TOC sub-settings (partially recognised) ───────────────────────
        "toc.useDottedLines" | "toc.captionText" | "toc.forwardLinks" | "toc.backLinks"
        | "toc.indentation" | "toc.fontScale" => {
            o.warnings.push(format!(
                "setting {name:?} is recognised but not yet implemented in this engine; ignored"
            ));
        }

        // ── Recognised-but-unimplemented ──────────────────────────────────
        "useExternalLinks"
        | "useLocalLinks"
        | "replacements"
        | "tocXsl"
        | "load.proxy"
        | "load.cookieJar"
        | "load.customHeaders"
        | "load.repeatCustomHeaders"
        | "load.cookies"
        | "load.post"
        | "load.blockLocalFileAccess"
        | "load.stopSlowScripts"
        | "load.debugJavascript"
        | "load.loadErrorHandling"
        | "load.mediaLoadErrorHandling"
        | "load.runScript"
        | "load.cacheDir"
        | "load.clientSslKeyPath"
        | "load.clientSslKeyPassword"
        | "load.clientSslCrtPath"
        | "web.enableIntelligentShrinking"
        | "web.minimumFontSize"
        | "web.defaultEncoding"
        | "web.userStyleSheet"
        | "web.enablePlugins"
        | "web.loadImages"
        | "header.htmlUrl"
        | "header.fontName"
        | "header.spacing"
        | "footer.htmlUrl"
        | "footer.fontName"
        | "footer.spacing" => {
            o.warnings.push(format!(
                "setting {name:?} is recognised but not yet implemented in this engine; ignored"
            ));
        }

        // ── Unknown ───────────────────────────────────────────────────────
        _ => {
            return Err(WkError::BadArg(format!("unknown setting: {name:?}")));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Getters
// ---------------------------------------------------------------------------

/// Return the current string representation of a global-settings field.
///
/// Returns `Some(value_string)` when `name` is a known wkhtmltopdf setting
/// (even if recognised-but-unimplemented — those return an empty string as the
/// default value).  Returns `None` for unknown names.
pub fn get_global(g: &GlobalSettings, name: &str) -> Option<String> {
    let v: String = match name {
        // ── size ──────────────────────────────────────────────────────────
        "size.pageSize" => named_page_size_as_str(g.page_size).to_owned(),
        "size.width" => g
            .page_width_mm
            .map(|mm| format!("{mm}mm"))
            .unwrap_or_default(),
        "size.height" => g
            .page_height_mm
            .map(|mm| format!("{mm}mm"))
            .unwrap_or_default(),

        // ── margins ───────────────────────────────────────────────────────
        "margin.top" => format!("{}mm", g.margin_top_mm),
        "margin.bottom" => format!("{}mm", g.margin_bottom_mm),
        "margin.left" => format!("{}mm", g.margin_left_mm),
        "margin.right" => format!("{}mm", g.margin_right_mm),

        // ── layout ────────────────────────────────────────────────────────
        "orientation" => match g.orientation {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Landscape",
        }
        .to_owned(),
        "dpi" => g.dpi.to_string(),
        "imageDPI" => g.image_dpi.to_string(),
        "imageQuality" => g.image_quality.to_string(),
        "colorMode" => match g.color_mode {
            ColorMode::Color => "color",
            ColorMode::Grayscale => "grayscale",
        }
        .to_owned(),
        "zoom" | "load.zoomFactor" => g.zoom.to_string(),

        // ── web rendering ─────────────────────────────────────────────────
        "web.printMediaType" => g.print_media_type.to_string(),
        "web.enableJavascript" => g.enable_javascript.to_string(),
        "web.background" => g.print_background.to_string(),

        // ── load ──────────────────────────────────────────────────────────
        "load.jsdelay" => g.javascript_delay_ms.to_string(),
        "load.proxy" => g.proxy.clone().unwrap_or_default(),
        "load.blockLocalFileAccess" => (!g.allow_local_file_access).to_string(),
        "load.username" => g.username.clone(),
        "load.password" => g.password.clone(),

        // ── Security policy ───────────────────────────────────────────────
        "load.safe" => g.safe_mode.to_string(),
        "load.allowedPath" => g.allowed_paths.join(","),
        "load.disableExternalLinks" => g.block_external_links.to_string(),
        "load.disableInternalLinks" => g.block_internal_links.to_string(),

        // ── header ────────────────────────────────────────────────────────
        "header.left" => g.header.left.clone(),
        "header.center" => g.header.center.clone(),
        "header.right" => g.header.right.clone(),
        "header.fontSize" => g.header.font_size.to_string(),
        "header.line" => g.header.line.to_string(),

        // ── footer ────────────────────────────────────────────────────────
        "footer.left" => g.footer.left.clone(),
        "footer.center" => g.footer.center.clone(),
        "footer.right" => g.footer.right.clone(),
        "footer.fontSize" => g.footer.font_size.to_string(),
        "footer.line" => g.footer.line.to_string(),

        // ── document structure ────────────────────────────────────────────
        "toc" => g.produce_toc.to_string(),
        "produceForms" => g.produce_forms.to_string(),
        "outline" => g.produce_outline.to_string(),
        "outlineDepth" => g.outline_depth.to_string(),
        "documentTitle" => g.document_title.clone(),
        "out" => g.output_path.clone(),
        "cover" => g.cover.clone().unwrap_or_default(),

        // ── TOC XSL / custom stylesheet ───────────────────────────────────
        "tocXsl" => g.toc_xsl.clone().unwrap_or_default(),
        "toc.captionText" => g.toc_settings.caption_text.clone(),
        "toc.useDottedLines" => g.toc_settings.use_dotted_lines.to_string(),
        "toc.forwardLinks" => g.toc_settings.forward_links.to_string(),
        "toc.backLinks" => g.toc_settings.back_links.to_string(),
        "toc.indentation" => g.toc_settings.indentation.clone(),
        "toc.fontScale" => g.toc_settings.font_scale.to_string(),

        // ── recognised-but-unimplemented → return empty default ───────────
        "logLevel"
        | "quiet"
        | "useGraphics"
        | "resolveRelativeLinks"
        | "resolution"
        | "pageOffset"
        | "copies"
        | "collate"
        | "dumpOutline"
        | "useCompression"
        | "viewportSize"
        | "load.cookieJar"
        | "web.enableIntelligentShrinking"
        | "web.minimumFontSize"
        | "web.defaultEncoding"
        | "web.userStyleSheet"
        | "web.enablePlugins"
        | "web.loadImages"
        | "header.htmlUrl"
        | "header.fontName"
        | "header.spacing"
        | "footer.htmlUrl"
        | "footer.fontName"
        | "footer.spacing" => String::new(),

        // ── unknown ───────────────────────────────────────────────────────
        _ => return None,
    };
    Some(v)
}

/// Return the current string representation of a per-object settings field.
///
/// Semantics mirror [`get_global`].
pub fn get_object(o: &PdfObjectSettings, name: &str) -> Option<String> {
    let v: String = match name {
        // ── source ────────────────────────────────────────────────────────
        "page" => o.page.clone(),

        // ── web ───────────────────────────────────────────────────────────
        "web.printMediaType" => o.print_media_type.to_string(),
        "web.enableJavascript" => o.enable_javascript.to_string(),
        "web.background" => o.print_background.to_string(),

        // ── load ──────────────────────────────────────────────────────────
        "load.jsdelay" => o.javascript_delay_ms.to_string(),
        "load.zoomFactor" => o.zoom.to_string(),
        "load.username" => o.username.clone(),
        "load.password" => o.password.clone(),

        // ── header ────────────────────────────────────────────────────────
        "header.left" => o.header.left.clone(),
        "header.center" => o.header.center.clone(),
        "header.right" => o.header.right.clone(),
        "header.fontSize" => o.header.font_size.to_string(),
        "header.line" => o.header.line.to_string(),

        // ── footer ────────────────────────────────────────────────────────
        "footer.left" => o.footer.left.clone(),
        "footer.center" => o.footer.center.clone(),
        "footer.right" => o.footer.right.clone(),
        "footer.fontSize" => o.footer.font_size.to_string(),
        "footer.line" => o.footer.line.to_string(),

        // ── structural flags ──────────────────────────────────────────────
        "includeInOutline" => o.include_in_outline.to_string(),
        "pagesCount" => o.pages_count.to_string(),
        "isTableOfContent" => o.is_table_of_content.to_string(),
        "produceForms" => o.produce_forms.to_string(),

        // ── recognised-but-unimplemented → return empty default ───────────
        "toc.useDottedLines"
        | "toc.captionText"
        | "toc.forwardLinks"
        | "toc.backLinks"
        | "toc.indentation"
        | "toc.fontScale"
        | "useExternalLinks"
        | "useLocalLinks"
        | "replacements"
        | "tocXsl"
        | "load.proxy"
        | "load.cookieJar"
        | "load.customHeaders"
        | "load.repeatCustomHeaders"
        | "load.cookies"
        | "load.post"
        | "load.blockLocalFileAccess"
        | "load.stopSlowScripts"
        | "load.debugJavascript"
        | "load.loadErrorHandling"
        | "load.mediaLoadErrorHandling"
        | "load.runScript"
        | "load.cacheDir"
        | "load.clientSslKeyPath"
        | "load.clientSslKeyPassword"
        | "load.clientSslCrtPath"
        | "web.enableIntelligentShrinking"
        | "web.minimumFontSize"
        | "web.defaultEncoding"
        | "web.userStyleSheet"
        | "web.enablePlugins"
        | "web.loadImages"
        | "header.htmlUrl"
        | "header.fontName"
        | "header.spacing"
        | "footer.htmlUrl"
        | "footer.fontName"
        | "footer.spacing" => String::new(),

        // ── unknown ───────────────────────────────────────────────────────
        _ => return None,
    };
    Some(v)
}

// ---------------------------------------------------------------------------
// Image global settings
// ---------------------------------------------------------------------------

/// Set an `ImageGlobalSettings` field by its upstream dotted name.
///
/// The naming follows the upstream wkhtmltoimage reflection system
/// (`imagesettings.cc` / `imagecommandlineparser.cc`).  The names
/// `"loadPage.*"` and `"load.*"` are both accepted as aliases for the
/// load/web settings, matching how wkhtmltopdf treats these names.
///
/// # Errors
/// Returns `Err(WkError::BadArg)` for an unknown name or an unparseable value.
pub fn set_image_global(g: &mut ImageGlobalSettings, name: &str, value: &str) -> Result<()> {
    match name {
        // ── input / output ────────────────────────────────────────────────
        "in" => {
            g.in_path = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "out" => {
            g.out = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "fmt" => {
            g.fmt = value.to_ascii_lowercase();
        }
        "quality" => {
            g.quality = value
                .trim()
                .parse::<u8>()
                .map_err(|_| WkError::BadArg(format!("invalid quality: {value:?}")))?;
        }

        // ── viewport / layout ─────────────────────────────────────────────
        "screenWidth" => {
            g.screen_width = Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| WkError::BadArg(format!("invalid screenWidth: {value:?}")))?,
            );
        }
        "screenHeight" => {
            g.screen_height = Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| WkError::BadArg(format!("invalid screenHeight: {value:?}")))?,
            );
        }
        "smartWidth" => {
            g.smart_width = parse_bool(value)?;
        }
        "zoom" | "load.zoomFactor" | "loadPage.zoomFactor" => {
            g.zoom = value
                .trim()
                .parse::<f64>()
                .map_err(|_| WkError::BadArg(format!("invalid zoom: {value:?}")))?;
        }

        // ── crop ──────────────────────────────────────────────────────────
        "crop.left" => {
            g.crop_x = Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| WkError::BadArg(format!("invalid crop.left: {value:?}")))?,
            );
        }
        "crop.top" => {
            g.crop_y = Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| WkError::BadArg(format!("invalid crop.top: {value:?}")))?,
            );
        }
        "crop.width" => {
            g.crop_w = Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| WkError::BadArg(format!("invalid crop.width: {value:?}")))?,
            );
        }
        "crop.height" => {
            g.crop_h = Some(
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| WkError::BadArg(format!("invalid crop.height: {value:?}")))?,
            );
        }

        // ── transparency ──────────────────────────────────────────────────
        "transparent" => {
            g.transparent = parse_bool(value)?;
        }

        // ── web rendering ─────────────────────────────────────────────────
        "web.enableJavascript" => g.enable_javascript = parse_bool(value)?,
        "web.printMediaType" => g.print_media_type = parse_bool(value)?,
        "web.background" => g.print_background = parse_bool(value)?,
        "web.loadImages" => g.load_images = parse_bool(value)?,

        // ── load (canonical + loadPage.* aliases) ─────────────────────────
        "load.jsdelay" | "loadPage.jsdelay" => {
            g.javascript_delay_ms = value
                .trim()
                .parse::<u64>()
                .map_err(|_| WkError::BadArg(format!("invalid jsdelay: {value:?}")))?;
        }
        "load.proxy" | "loadPage.proxy" => {
            g.proxy = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        "load.blockLocalFileAccess" | "loadPage.blockLocalFileAccess" => {
            g.allow_local_file_access = !parse_bool(value)?;
        }
        "load.username" | "loadPage.username" => g.username = value.to_string(),
        "load.password" | "loadPage.password" => g.password = value.to_string(),
        "load.noCheckCertificate" => g.no_check_certificate = parse_bool(value)?,

        // ── security policy ───────────────────────────────────────────────
        "load.safe" => g.safe_mode = parse_bool(value)?,
        "load.allowedPath" => {
            if !value.is_empty() {
                g.allowed_paths.push(value.to_string());
            }
        }
        "load.disableExternalLinks" => g.block_external_links = parse_bool(value)?,
        "load.disableInternalLinks" => g.block_internal_links = parse_bool(value)?,

        // ── recognised-but-unimplemented ──────────────────────────────────
        "logLevel"
        | "quiet"
        | "useGraphics"
        | "loadGlobal.cookieJar"
        | "load.cookieJar"
        | "web.enableIntelligentShrinking"
        | "web.minimumFontSize"
        | "web.defaultEncoding"
        | "web.userStyleSheet"
        | "web.enablePlugins"
        | "loadPage.windowStatus"
        | "loadPage.stopSlowScripts"
        | "loadPage.debugJavascript"
        | "loadPage.loadErrorHandling"
        | "loadPage.runScript"
        | "loadPage.cacheDir"
        | "loadPage.clientSslKeyPath"
        | "loadPage.clientSslKeyPassword"
        | "loadPage.clientSslCrtPath"
        | "loadPage.repeatCustomHeaders"
        | "loadPage.cookieJar" => {
            g.warnings.push(format!(
                "setting {name:?} is recognised but not yet implemented in this engine; ignored"
            ));
        }

        // ── unknown ───────────────────────────────────────────────────────
        _ => {
            return Err(WkError::BadArg(format!("unknown image setting: {name:?}")));
        }
    }
    Ok(())
}

/// Return the current string representation of an image-settings field.
///
/// Returns `Some(value_string)` for a known name (even recognised-but-
/// unimplemented — those return an empty string).  Returns `None` for unknown
/// names.
pub fn get_image_global(g: &ImageGlobalSettings, name: &str) -> Option<String> {
    let v: String = match name {
        // ── input / output ────────────────────────────────────────────────
        "in" => g.in_path.clone().unwrap_or_default(),
        "out" => g.out.clone().unwrap_or_default(),
        "fmt" => g.fmt.clone(),
        "quality" => g.quality.to_string(),

        // ── viewport / layout ─────────────────────────────────────────────
        "screenWidth" => g.screen_width.map(|v| v.to_string()).unwrap_or_default(),
        "screenHeight" => g.screen_height.map(|v| v.to_string()).unwrap_or_default(),
        "smartWidth" => g.smart_width.to_string(),
        "zoom" | "load.zoomFactor" | "loadPage.zoomFactor" => g.zoom.to_string(),

        // ── crop ──────────────────────────────────────────────────────────
        "crop.left" => g.crop_x.map(|v| v.to_string()).unwrap_or_default(),
        "crop.top" => g.crop_y.map(|v| v.to_string()).unwrap_or_default(),
        "crop.width" => g.crop_w.map(|v| v.to_string()).unwrap_or_default(),
        "crop.height" => g.crop_h.map(|v| v.to_string()).unwrap_or_default(),

        // ── transparency ──────────────────────────────────────────────────
        "transparent" => g.transparent.to_string(),

        // ── web rendering ─────────────────────────────────────────────────
        "web.enableJavascript" => g.enable_javascript.to_string(),
        "web.printMediaType" => g.print_media_type.to_string(),
        "web.background" => g.print_background.to_string(),
        "web.loadImages" => g.load_images.to_string(),

        // ── load ──────────────────────────────────────────────────────────
        "load.jsdelay" | "loadPage.jsdelay" => g.javascript_delay_ms.to_string(),
        "load.proxy" | "loadPage.proxy" => g.proxy.clone().unwrap_or_default(),
        "load.blockLocalFileAccess" | "loadPage.blockLocalFileAccess" => {
            (!g.allow_local_file_access).to_string()
        }
        "load.username" | "loadPage.username" => g.username.clone(),
        "load.password" | "loadPage.password" => g.password.clone(),
        "load.noCheckCertificate" => g.no_check_certificate.to_string(),

        // ── security policy ───────────────────────────────────────────────
        "load.safe" => g.safe_mode.to_string(),
        "load.allowedPath" => g.allowed_paths.join(","),
        "load.disableExternalLinks" => g.block_external_links.to_string(),
        "load.disableInternalLinks" => g.block_internal_links.to_string(),

        // ── recognised-but-unimplemented → empty default ──────────────────
        "logLevel"
        | "quiet"
        | "useGraphics"
        | "loadGlobal.cookieJar"
        | "load.cookieJar"
        | "web.enableIntelligentShrinking"
        | "web.minimumFontSize"
        | "web.defaultEncoding"
        | "web.userStyleSheet"
        | "web.enablePlugins"
        | "loadPage.windowStatus"
        | "loadPage.stopSlowScripts"
        | "loadPage.debugJavascript"
        | "loadPage.loadErrorHandling"
        | "loadPage.runScript"
        | "loadPage.cacheDir"
        | "loadPage.clientSslKeyPath"
        | "loadPage.clientSslKeyPassword"
        | "loadPage.clientSslCrtPath"
        | "loadPage.repeatCustomHeaders"
        | "loadPage.cookieJar" => String::new(),

        // ── unknown ───────────────────────────────────────────────────────
        _ => return None,
    };
    Some(v)
}

/// Map a `NamedPageSize` to its canonical wkhtmltopdf string representation.
fn named_page_size_as_str(ps: NamedPageSize) -> &'static str {
    match ps {
        NamedPageSize::A0 => "A0",
        NamedPageSize::A1 => "A1",
        NamedPageSize::A2 => "A2",
        NamedPageSize::A3 => "A3",
        NamedPageSize::A4 => "A4",
        NamedPageSize::A5 => "A5",
        NamedPageSize::A6 => "A6",
        NamedPageSize::A7 => "A7",
        NamedPageSize::A8 => "A8",
        NamedPageSize::A9 => "A9",
        NamedPageSize::B0 => "B0",
        NamedPageSize::B1 => "B1",
        NamedPageSize::B2 => "B2",
        NamedPageSize::B3 => "B3",
        NamedPageSize::B4 => "B4",
        NamedPageSize::B5 => "B5",
        NamedPageSize::B6 => "B6",
        NamedPageSize::B7 => "B7",
        NamedPageSize::B8 => "B8",
        NamedPageSize::B9 => "B9",
        NamedPageSize::B10 => "B10",
        NamedPageSize::C5E => "C5E",
        NamedPageSize::Comm10E => "Comm10E",
        NamedPageSize::DLE => "DLE",
        NamedPageSize::Executive => "Executive",
        NamedPageSize::Folio => "Folio",
        NamedPageSize::Ledger => "Ledger",
        NamedPageSize::Legal => "Legal",
        NamedPageSize::Letter => "Letter",
        NamedPageSize::Tabloid => "Tabloid",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Orientation;
    use crate::settings::{GlobalSettings, NamedPageSize, PdfObjectSettings};

    /// Round-trip: set a representative cross-section of global settings and
    /// verify that the fields and the derived geometry/opts reflect them.
    #[test]
    fn global_round_trip() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "size.pageSize", "A4").unwrap();
        set_global(&mut g, "margin.top", "15mm").unwrap();
        set_global(&mut g, "orientation", "Landscape").unwrap();
        set_global(&mut g, "load.jsdelay", "200").unwrap();
        set_global(&mut g, "header.center", "[page]/[topage]").unwrap();
        set_global(&mut g, "toc", "true").unwrap();

        // Fields
        assert_eq!(g.page_size, NamedPageSize::A4);
        assert!((g.margin_top_mm - 15.0).abs() < 1e-9);
        assert!(matches!(g.orientation, Orientation::Landscape));
        assert_eq!(g.javascript_delay_ms, 200);
        assert_eq!(g.header.center, "[page]/[topage]");
        assert!(g.produce_toc);
        assert!(g.warnings.is_empty(), "no warnings expected");

        // to_geometry()
        let geom = g.to_geometry();
        // Landscape A4: width=297, height=210
        assert!((geom.width_mm - 297.0).abs() < 1e-9);
        assert!((geom.height_mm - 210.0).abs() < 1e-9);
        assert!((geom.margin_top_mm - 15.0).abs() < 1e-9);
        assert!(matches!(geom.orientation, Orientation::Landscape));

        // to_assemble_opts()
        let opts = g.to_assemble_opts();
        assert!(opts.with_toc);
        let header = opts.header.expect("header should be Some");
        assert_eq!(header.center, "[page]/[topage]");
    }

    /// Additional margin/size settings in a single pass.
    #[test]
    fn global_margins_and_size() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "size.width", "200mm").unwrap();
        set_global(&mut g, "size.height", "280mm").unwrap();
        set_global(&mut g, "margin.bottom", "20mm").unwrap();
        set_global(&mut g, "margin.left", "15mm").unwrap();
        set_global(&mut g, "margin.right", "15mm").unwrap();

        let geom = g.to_geometry();
        assert!((geom.width_mm - 200.0).abs() < 1e-9);
        assert!((geom.height_mm - 280.0).abs() < 1e-9);
        assert!((geom.margin_bottom_mm - 20.0).abs() < 1e-9);
    }

    /// Footer-only settings flow into to_assemble_opts correctly.
    #[test]
    fn global_footer_only() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "footer.center", "[page]").unwrap();
        set_global(&mut g, "footer.fontSize", "10").unwrap();

        let opts = g.to_assemble_opts();
        assert!(opts.header.is_none());
        let footer = opts.footer.expect("footer should be Some");
        assert_eq!(footer.center, "[page]");
        assert!((opts.header_footer_font_size - 10.0).abs() < 1e-9);
    }

    /// Cover URL is forwarded.
    #[test]
    fn global_cover() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "cover", "https://example.com/cover.html").unwrap();
        let opts = g.to_assemble_opts();
        assert!(opts.cover.is_some());
    }

    /// Empty cover value clears the cover.
    #[test]
    fn global_cover_empty_clears() {
        let mut g = GlobalSettings::default();
        g.cover = Some("old".into());
        set_global(&mut g, "cover", "").unwrap();
        assert!(g.cover.is_none());
    }

    /// Unknown name → Err(BadArg).
    #[test]
    fn unknown_name_errors() {
        let mut g = GlobalSettings::default();
        let err = set_global(&mut g, "totally.unknown.xyz", "1").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    /// A recognised-but-unimplemented name → Ok + warning recorded.
    #[test]
    fn recognised_unimplemented_global_warns() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "load.cookieJar", "/tmp/cookies").unwrap();
        assert!(
            !g.warnings.is_empty(),
            "expected a warning for load.cookieJar"
        );
        assert!(
            g.warnings[0].contains("load.cookieJar"),
            "warning should mention the setting name"
        );
    }

    /// Bad value → Err(BadArg) (not a panic).
    #[test]
    fn bad_value_gives_bad_arg_error() {
        let mut g = GlobalSettings::default();
        let err = set_global(&mut g, "dpi", "not-a-number").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    /// Unknown page size name → Err(BadArg).
    #[test]
    fn unknown_page_size_errors() {
        let mut g = GlobalSettings::default();
        let err = set_global(&mut g, "size.pageSize", "Quux").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    /// Unknown orientation → Err(BadArg).
    #[test]
    fn unknown_orientation_errors() {
        let mut g = GlobalSettings::default();
        let err = set_global(&mut g, "orientation", "sideways").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    // --- Object settings ---

    /// Round-trip for set_object.
    #[test]
    fn object_round_trip() {
        let mut o = PdfObjectSettings::default();
        set_object(&mut o, "page", "https://example.com").unwrap();
        set_object(&mut o, "web.enableJavascript", "false").unwrap();
        set_object(&mut o, "load.jsdelay", "500").unwrap();
        set_object(&mut o, "header.center", "Title").unwrap();
        set_object(&mut o, "includeInOutline", "false").unwrap();

        assert_eq!(o.page, "https://example.com");
        assert!(!o.enable_javascript);
        assert_eq!(o.javascript_delay_ms, 500);
        assert_eq!(o.header.center, "Title");
        assert!(!o.include_in_outline);
        assert!(o.warnings.is_empty());
    }

    /// Recognised-but-unimplemented object setting → Ok + warning.
    #[test]
    fn recognised_unimplemented_object_warns() {
        let mut o = PdfObjectSettings::default();
        set_object(&mut o, "useExternalLinks", "true").unwrap();
        assert!(!o.warnings.is_empty());
        assert!(o.warnings[0].contains("useExternalLinks"));
    }

    /// Unknown object setting → Err.
    #[test]
    fn unknown_object_name_errors() {
        let mut o = PdfObjectSettings::default();
        let err = set_object(&mut o, "no.such.setting", "x").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    /// TOC sub-settings → Ok + warning.
    #[test]
    fn toc_subsettings_warn() {
        let mut o = PdfObjectSettings::default();
        set_object(&mut o, "toc.useDottedLines", "false").unwrap();
        assert!(!o.warnings.is_empty());
    }

    // --- Getters ---

    #[test]
    fn get_global_known_name_returns_value() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "size.pageSize", "Letter").unwrap();
        assert_eq!(get_global(&g, "size.pageSize"), Some("Letter".to_owned()));
    }

    #[test]
    fn get_global_unknown_name_returns_none() {
        let g = GlobalSettings::default();
        assert_eq!(get_global(&g, "no.such.thing"), None);
    }

    #[test]
    fn get_global_unimplemented_name_returns_empty_string() {
        let g = GlobalSettings::default();
        assert_eq!(get_global(&g, "logLevel"), Some(String::new()));
    }

    #[test]
    fn get_global_margin_roundtrip() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "margin.top", "20mm").unwrap();
        // get_global returns "20mm" (stored as f64 20.0)
        let v = get_global(&g, "margin.top").unwrap();
        assert!(v.contains("20"), "expected '20' in '{v}'");
    }

    #[test]
    fn get_object_page_roundtrip() {
        let mut o = PdfObjectSettings::default();
        set_object(&mut o, "page", "https://example.com").unwrap();
        assert_eq!(
            get_object(&o, "page"),
            Some("https://example.com".to_owned())
        );
    }

    #[test]
    fn get_object_unknown_returns_none() {
        let o = PdfObjectSettings::default();
        assert_eq!(get_object(&o, "totally.unknown"), None);
    }

    // ── TOC XSL registry settings ─────────────────────────────────────────

    /// `toc.fontScale` is stored and flows into AssembleOpts.
    #[test]
    fn toc_font_scale_stored_and_threads_to_assemble_opts() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "toc.fontScale", "0.5").unwrap();
        assert!(
            (g.toc_settings.font_scale - 0.5).abs() < 1e-9,
            "toc_settings.font_scale should be 0.5"
        );
        assert!(g.warnings.is_empty(), "no warnings expected");
        let opts = g.to_assemble_opts();
        assert!(
            (opts.toc_settings.font_scale - 0.5).abs() < 1e-9,
            "AssembleOpts.toc_settings.font_scale should be 0.5"
        );
    }

    /// `tocXsl` is stored and flows into AssembleOpts.
    #[test]
    fn toc_xsl_stored_and_threads_to_assemble_opts() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "tocXsl", "/x.xsl").unwrap();
        assert_eq!(g.toc_xsl, Some("/x.xsl".to_owned()));
        assert!(g.warnings.is_empty(), "no warnings expected");
        let opts = g.to_assemble_opts();
        assert_eq!(opts.toc_xsl, Some("/x.xsl".to_owned()));
    }

    /// `tocXsl` empty string clears the path.
    #[test]
    fn toc_xsl_empty_clears() {
        let mut g = GlobalSettings::default();
        g.toc_xsl = Some("old.xsl".into());
        set_global(&mut g, "tocXsl", "").unwrap();
        assert!(g.toc_xsl.is_none());
    }

    /// `toc.captionText` is stored correctly.
    #[test]
    fn toc_caption_text_stored() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "toc.captionText", "Contents").unwrap();
        assert_eq!(g.toc_settings.caption_text, "Contents");
        assert_eq!(get_global(&g, "toc.captionText"), Some("Contents".to_owned()));
    }

    /// `toc.useDottedLines` is stored as bool.
    #[test]
    fn toc_dotted_lines_stored() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "toc.useDottedLines", "false").unwrap();
        assert!(!g.toc_settings.use_dotted_lines);
    }

    /// `toc.forwardLinks` / `toc.backLinks` are stored as bool.
    #[test]
    fn toc_link_settings_stored() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "toc.forwardLinks", "false").unwrap();
        set_global(&mut g, "toc.backLinks", "true").unwrap();
        assert!(!g.toc_settings.forward_links);
        assert!(g.toc_settings.back_links);
    }

    /// `toc.indentation` is stored as string.
    #[test]
    fn toc_indentation_stored() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "toc.indentation", "2em").unwrap();
        assert_eq!(g.toc_settings.indentation, "2em");
    }

    /// Bad `toc.fontScale` value → BadArg error.
    #[test]
    fn toc_font_scale_bad_value_errors() {
        let mut g = GlobalSettings::default();
        let err = set_global(&mut g, "toc.fontScale", "notanumber").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    /// take_warnings drains the list.
    #[test]
    fn take_warnings_drains() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "logLevel", "warn").unwrap();
        let w = g.take_warnings();
        assert!(!w.is_empty());
        assert!(g.warnings.is_empty(), "warnings should be drained");
    }

    /// Length units round-trip through set_global margins.
    #[test]
    fn set_global_margin_units() {
        let mut g = GlobalSettings::default();
        set_global(&mut g, "margin.top", "1in").unwrap();
        assert!((g.margin_top_mm - 25.4).abs() < 1e-9);

        set_global(&mut g, "margin.bottom", "2cm").unwrap();
        assert!((g.margin_bottom_mm - 20.0).abs() < 1e-9);

        set_global(&mut g, "margin.left", "72pt").unwrap();
        assert!((g.margin_left_mm - 25.4).abs() < 1e-9);

        set_global(&mut g, "margin.right", "96px").unwrap();
        assert!((g.margin_right_mm - 25.4).abs() < 1e-9);
    }

    // -----------------------------------------------------------------------
    // Image global settings tests
    // -----------------------------------------------------------------------

    /// Round-trip for set_image_global / get_image_global.
    #[test]
    fn image_global_round_trip() {
        let mut g = ImageGlobalSettings::default();
        set_image_global(&mut g, "fmt", "jpeg").unwrap();
        set_image_global(&mut g, "quality", "80").unwrap();
        set_image_global(&mut g, "screenWidth", "1920").unwrap();
        set_image_global(&mut g, "transparent", "true").unwrap();
        set_image_global(&mut g, "crop.left", "10").unwrap();
        set_image_global(&mut g, "crop.top", "20").unwrap();
        set_image_global(&mut g, "crop.width", "800").unwrap();
        set_image_global(&mut g, "crop.height", "600").unwrap();
        set_image_global(&mut g, "zoom", "1.5").unwrap();
        set_image_global(&mut g, "load.jsdelay", "500").unwrap();
        set_image_global(&mut g, "web.enableJavascript", "false").unwrap();

        assert_eq!(g.fmt, "jpeg");
        assert_eq!(g.quality, 80);
        assert_eq!(g.screen_width, Some(1920));
        assert!(g.transparent);
        assert_eq!(g.crop_x, Some(10));
        assert_eq!(g.crop_y, Some(20));
        assert_eq!(g.crop_w, Some(800));
        assert_eq!(g.crop_h, Some(600));
        assert!((g.zoom - 1.5).abs() < 1e-9);
        assert_eq!(g.javascript_delay_ms, 500);
        assert!(!g.enable_javascript);
        assert!(g.warnings.is_empty());

        // Verify get_image_global round-trips.
        assert_eq!(get_image_global(&g, "fmt"), Some("jpeg".to_owned()));
        assert_eq!(get_image_global(&g, "quality"), Some("80".to_owned()));
        assert_eq!(get_image_global(&g, "crop.left"), Some("10".to_owned()));
        assert_eq!(get_image_global(&g, "transparent"), Some("true".to_owned()));
    }

    /// set_image_global("in", ...) stores in_path.
    #[test]
    fn image_global_in_setting() {
        let mut g = ImageGlobalSettings::default();
        set_image_global(&mut g, "in", "https://example.com").unwrap();
        assert_eq!(g.in_path, Some("https://example.com".to_owned()));
        assert_eq!(
            get_image_global(&g, "in"),
            Some("https://example.com".to_owned())
        );

        set_image_global(&mut g, "in", "").unwrap(); // clear
        assert_eq!(g.in_path, None);
    }

    /// Unknown image setting name → Err.
    #[test]
    fn image_global_unknown_name_errors() {
        let mut g = ImageGlobalSettings::default();
        let err = set_image_global(&mut g, "totally.unknown.xyz", "v").unwrap_err();
        assert!(matches!(err, WkError::BadArg(_)));
    }

    /// Recognised-but-unimplemented image setting → Ok + warning.
    #[test]
    fn image_global_unimplemented_warns() {
        let mut g = ImageGlobalSettings::default();
        set_image_global(&mut g, "logLevel", "info").unwrap();
        assert!(!g.warnings.is_empty());
        assert!(g.warnings[0].contains("logLevel"));
    }

    /// Unknown image setting get → None.
    #[test]
    fn image_global_unknown_get_returns_none() {
        let g = ImageGlobalSettings::default();
        assert_eq!(get_image_global(&g, "no.such.thing"), None);
    }

    /// to_image_opts maps fields correctly.
    #[test]
    fn image_global_to_image_opts() {
        let mut g = ImageGlobalSettings::default();
        set_image_global(&mut g, "fmt", "jpeg").unwrap();
        set_image_global(&mut g, "quality", "75").unwrap();
        set_image_global(&mut g, "transparent", "false").unwrap();
        set_image_global(&mut g, "crop.left", "5").unwrap();
        set_image_global(&mut g, "crop.top", "10").unwrap();
        set_image_global(&mut g, "crop.width", "100").unwrap();
        set_image_global(&mut g, "crop.height", "200").unwrap();

        let opts = g.to_image_opts();
        assert!(matches!(opts.format, crate::render::ImageFormat::Jpeg));
        assert_eq!(opts.quality, 75);
        assert!(!opts.transparent);
        assert_eq!(opts.crop, Some((5, 10, 100, 200)));
    }

    /// Partial crop (missing fields) → no crop applied.
    #[test]
    fn image_global_partial_crop_no_crop() {
        let mut g = ImageGlobalSettings::default();
        set_image_global(&mut g, "crop.left", "5").unwrap();
        set_image_global(&mut g, "crop.top", "10").unwrap();
        // crop.width and crop.height not set

        let opts = g.to_image_opts();
        assert_eq!(opts.crop, None, "partial crop should result in no crop");
    }

    /// loadPage.* aliases work the same as load.*.
    #[test]
    fn image_global_loadpage_aliases() {
        let mut g = ImageGlobalSettings::default();
        set_image_global(&mut g, "loadPage.jsdelay", "300").unwrap();
        set_image_global(&mut g, "loadPage.username", "alice").unwrap();
        set_image_global(&mut g, "loadPage.zoomFactor", "2.0").unwrap();

        assert_eq!(g.javascript_delay_ms, 300);
        assert_eq!(g.username, "alice");
        assert!((g.zoom - 2.0).abs() < 1e-9);
    }
}
