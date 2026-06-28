// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//! Legacy-compatibility CSS profiles.

/// UA-reset stylesheet that nudges Chromium's print output toward the
/// wkhtmltopdf 0.12.6 / Qt4-WebKit baseline.
///
/// Targets: body margin, base font/line-height, heading sizes and margins,
/// block-level element margins, and list padding — the main sources of
/// element-spacing divergence that cause +1 page drift on plain-prose
/// and heading-heavy documents.
pub const WK0126_UA_RESET: &str = "\
/* wkhtmltox-rs legacy-compat (wk0126) */\n\
html{-webkit-text-size-adjust:100%;text-size-adjust:100%}\n\
body{margin:8px;font-family:serif;font-size:16px;line-height:1.12}\n\
h1{font-size:2em;margin:.67em 0}h2{font-size:1.5em;margin:.75em 0}h3{font-size:1.17em;margin:.83em 0}\n\
h4{margin:1.12em 0}h5{font-size:.83em;margin:1.5em 0}h6{font-size:.75em;margin:1.67em 0}\n\
p,blockquote,ul,ol,dl,table{margin:1em 0}ul,ol{padding-left:40px}\n\
*{-webkit-print-color-adjust:exact;print-color-adjust:exact}\n\
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::LoadSettings;

    #[test]
    fn wk0126_ua_reset_is_non_empty() {
        assert!(!WK0126_UA_RESET.is_empty());
    }

    #[test]
    fn load_settings_compat_ua_css_defaults_to_none() {
        assert!(LoadSettings::default().compat_ua_css.is_none());
    }
}
