// wkhtmltox-rs — Copyright 2026 wkhtmltopdf authors. LGPL-3.0-or-later.
//
// Core image pipeline: decode a raw CDP screenshot, apply resize/crop/alpha
// flattening, and re-encode as PNG or JPEG.
//
// The `image` crate is aliased as `img` to avoid a collision with this
// module's own name (`pub mod image` in lib.rs).
use ::image as img;
use std::io::Cursor;

use crate::error::Result;
use crate::render::{ImageFormat, RawImage};
use crate::WkError;

/// Options for the post-capture image pipeline.
#[derive(Debug, Clone)]
pub struct ImageOpts {
    /// Output format.
    pub format: ImageFormat,
    /// Target width in pixels.  `None` = keep original (or preserve aspect
    /// when only `height` is set).
    pub width: Option<u32>,
    /// Target height in pixels.  `None` = keep original (or preserve aspect
    /// when only `width` is set).
    pub height: Option<u32>,
    /// JPEG quality (0–100).  Ignored for PNG output.
    pub quality: u8,
    /// When `true`, keep the alpha channel in PNG output.  When `false`
    /// (default) the alpha is composited onto a white background, matching
    /// wkhtmltoimage's default behaviour.
    pub transparent: bool,
    /// Pixel-level crop applied AFTER decoding (x, y, width, height).
    /// The CDP `clip` parameter handles this upstream during `snapshot()`; this
    /// field is a fallback for callers that skipped the CDP clip.
    pub crop: Option<(u32, u32, u32, u32)>,
    /// Zoom factor (informational; actual scaling is handled by the CDP
    /// viewport / DeviceScaleFactor before the screenshot is taken).
    pub zoom: f64,
    /// Hint for the screen/viewport width used during rendering.
    pub screen_width: Option<u32>,
}

impl Default for ImageOpts {
    fn default() -> Self {
        Self {
            format: ImageFormat::Png,
            width: None,
            height: None,
            quality: 94,
            transparent: false,
            crop: None,
            zoom: 1.0,
            screen_width: None,
        }
    }
}

/// Composite an RGBA image over a solid-white background, returning an RGB image.
///
/// Used by both the PNG non-transparent path and the JPEG path so that
/// transparency always flattens to white rather than black.
fn composite_over_white(rgba: &img::RgbaImage) -> img::RgbImage {
    let (w, h) = rgba.dimensions();
    let mut flat = img::RgbImage::new(w, h);
    for (x, y, pix) in rgba.enumerate_pixels() {
        let a = pix[3] as f32 / 255.0;
        let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        flat.put_pixel(x, y, img::Rgb([blend(pix[0]), blend(pix[1]), blend(pix[2])]));
    }
    flat
}

/// Process a raw CDP screenshot into the final output image.
///
/// Pipeline steps (in order):
/// 1. Decode `raw.bytes` from the format indicated by `raw.format`.
/// 2. Apply pixel crop (`opts.crop`) if set.
/// 3. Resize to `opts.width` / `opts.height`:
///    - Both set → exact resize (may distort aspect ratio).
///    - Only `width` set → scale preserving aspect ratio.
///    - Only `height` set → scale preserving aspect ratio.
///    - Neither set → no resize.
/// 4. If `format == Png && !transparent` → flatten alpha onto white.
/// 5. Re-encode to PNG (lossless) or JPEG at `opts.quality`.
///
/// Returns the encoded bytes.
pub fn produce(raw: &RawImage, opts: &ImageOpts) -> Result<Vec<u8>> {
    // ── Step 1: decode ────────────────────────────────────────────────────────
    let mut dynimg = img::load_from_memory(&raw.bytes)
        .map_err(|e| WkError::Render(format!("image decode: {e}")))?;

    // ── Step 2: crop ──────────────────────────────────────────────────────────
    if let Some((x, y, w, h)) = opts.crop {
        dynimg = dynimg.crop_imm(x, y, w, h);
    }

    // ── Step 3: resize ────────────────────────────────────────────────────────
    dynimg = match (opts.width, opts.height) {
        (Some(w), Some(h)) => {
            dynimg.resize_exact(w, h, img::imageops::FilterType::Lanczos3)
        }
        (Some(w), None) => {
            dynimg.resize(w, u32::MAX, img::imageops::FilterType::Lanczos3)
        }
        (None, Some(h)) => {
            dynimg.resize(u32::MAX, h, img::imageops::FilterType::Lanczos3)
        }
        (None, None) => dynimg,
    };

    // ── Steps 4 + 5: encode ───────────────────────────────────────────────────
    let mut buf = Cursor::new(Vec::new());
    match opts.format {
        ImageFormat::Png => {
            let to_encode = if !opts.transparent {
                // Composite alpha onto solid white.
                img::DynamicImage::ImageRgb8(composite_over_white(&dynimg.to_rgba8()))
            } else {
                dynimg
            };
            to_encode
                .write_to(&mut buf, img::ImageFormat::Png)
                .map_err(|e| WkError::Render(format!("png encode: {e}")))?;
        }
        ImageFormat::Jpeg => {
            // JPEG has no alpha channel; composite over white (matches the PNG
            // non-transparent path) so transparency flattens to white, never black.
            let rgb = img::DynamicImage::ImageRgb8(composite_over_white(&dynimg.to_rgba8()));
            let encoder = img::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut buf,
                opts.quality,
            );
            rgb.write_with_encoder(encoder)
                .map_err(|e| WkError::Render(format!("jpeg encode: {e}")))?;
        }
    }
    Ok(buf.into_inner())
}

/// Encode a minimal valid PNG from a 2×2 RGBA pixel buffer.
///
/// Exported for `testing::MockRenderer::snapshot` so that mock output is a
/// real parseable PNG rather than arbitrary bytes.
pub fn tiny_png() -> Vec<u8> {
    let mut imgbuf = img::RgbaImage::new(2, 2);
    imgbuf.put_pixel(0, 0, img::Rgba([255,   0,   0, 255])); // opaque red
    imgbuf.put_pixel(1, 0, img::Rgba([  0, 255,   0, 128])); // semi green
    imgbuf.put_pixel(0, 1, img::Rgba([  0,   0, 255,  64])); // translucent blue
    imgbuf.put_pixel(1, 1, img::Rgba([255, 255, 255,   0])); // transparent white
    let dyn_img = img::DynamicImage::ImageRgba8(imgbuf);
    let mut bytes: Vec<u8> = Vec::new();
    let mut cursor = Cursor::new(&mut bytes);
    // Infallible for a known-good small image.
    dyn_img
        .write_to(&mut cursor, img::ImageFormat::Png)
        .expect("tiny_png: encode failed");
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image as img;

    /// Build a 100×100 RGBA test image and return it as PNG bytes.
    fn make_png_100x100() -> Vec<u8> {
        let mut imgbuf = img::RgbaImage::new(100, 100);
        for (x, y, pix) in imgbuf.enumerate_pixels_mut() {
            *pix = img::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 200]);
        }
        let dynimg = img::DynamicImage::ImageRgba8(imgbuf);
        let mut buf = Cursor::new(Vec::new());
        dynimg.write_to(&mut buf, img::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// Build a fully-transparent 40×40 RGBA image as PNG bytes.
    fn make_transparent_png() -> Vec<u8> {
        let imgbuf = img::RgbaImage::new(40, 40); // all pixels default to (0,0,0,0)
        let dynimg = img::DynamicImage::ImageRgba8(imgbuf);
        let mut buf = Cursor::new(Vec::new());
        dynimg.write_to(&mut buf, img::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn produce_png_resize_to_width() {
        let png_bytes = make_png_100x100();
        let raw = RawImage { bytes: png_bytes, format: ImageFormat::Png };
        let opts = ImageOpts { width: Some(50), ..Default::default() };
        let out = produce(&raw, &opts).expect("produce");

        // Must start with PNG magic.
        assert_eq!(&out[..4], b"\x89PNG", "output must be PNG");

        // Decode and verify dimensions.
        let decoded = img::load_from_memory(&out).expect("decode output");
        assert_eq!(decoded.width(), 50, "width must be 50");
        // Height is aspect-preserved from 100×100 → 50×50.
        assert_eq!(decoded.height(), 50, "height must be aspect-preserved");
    }

    #[test]
    fn produce_jpeg_format() {
        let png_bytes = make_png_100x100();
        let raw = RawImage { bytes: png_bytes, format: ImageFormat::Png };
        let opts = ImageOpts {
            format: ImageFormat::Jpeg,
            quality: 80,
            ..Default::default()
        };
        let out = produce(&raw, &opts).expect("produce jpeg");
        // JPEG magic bytes.
        assert_eq!(&out[..2], b"\xff\xd8", "output must be JPEG");
    }

    #[test]
    fn produce_transparent_false_flattens_alpha() {
        let png_bytes = make_transparent_png();
        let raw = RawImage { bytes: png_bytes, format: ImageFormat::Png };
        let opts = ImageOpts {
            transparent: false,
            ..Default::default()
        };
        let out = produce(&raw, &opts).expect("produce with alpha flatten");
        assert_eq!(&out[..4], b"\x89PNG", "output must be PNG");

        // Decode and verify all pixels are fully opaque (no alpha).
        let decoded = img::load_from_memory(&out).expect("decode output");
        let rgb = decoded.to_rgb8(); // RGB8 has no alpha channel
        for (_, _, pix) in rgb.enumerate_pixels() {
            // All pixels composited from transparent → white (255,255,255).
            assert_eq!(pix[0], 255, "red channel must be white");
            assert_eq!(pix[1], 255, "green channel must be white");
            assert_eq!(pix[2], 255, "blue channel must be white");
        }
    }

    #[test]
    fn produce_both_width_and_height_exact() {
        let png_bytes = make_png_100x100();
        let raw = RawImage { bytes: png_bytes, format: ImageFormat::Png };
        let opts = ImageOpts { width: Some(30), height: Some(40), ..Default::default() };
        let out = produce(&raw, &opts).expect("produce exact resize");
        let decoded = img::load_from_memory(&out).expect("decode");
        assert_eq!(decoded.width(), 30);
        assert_eq!(decoded.height(), 40);
    }

    #[test]
    fn produce_crop_then_resize() {
        let png_bytes = make_png_100x100();
        let raw = RawImage { bytes: png_bytes, format: ImageFormat::Png };
        // Crop to 60×60 at (10,10), then resize width to 30.
        let opts = ImageOpts {
            crop: Some((10, 10, 60, 60)),
            width: Some(30),
            ..Default::default()
        };
        let out = produce(&raw, &opts).expect("produce crop+resize");
        let decoded = img::load_from_memory(&out).expect("decode");
        assert_eq!(decoded.width(), 30);
        assert_eq!(decoded.height(), 30); // 60×60 → 30×30
    }

    #[test]
    fn tiny_png_is_valid_png() {
        let bytes = tiny_png();
        assert_eq!(&bytes[..4], b"\x89PNG");
        let img = img::load_from_memory(&bytes).expect("decode tiny_png");
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn jpeg_flattens_alpha_over_white_not_black() {
        // A fully transparent RGBA pixel must encode as white (255), not black,
        // on the JPEG path (JPEG has no alpha; raw to_rgb8 would drop to 0,0,0).
        let mut rgba = img::RgbaImage::new(2, 2);
        for px in rgba.pixels_mut() { *px = img::Rgba([0, 0, 0, 0]); } // transparent black
        let mut png_buf = Cursor::new(Vec::new());
        img::DynamicImage::ImageRgba8(rgba)
            .write_to(&mut png_buf, img::ImageFormat::Png)
            .unwrap();
        let raw = RawImage { bytes: png_buf.into_inner(), format: ImageFormat::Png };
        let opts = ImageOpts { format: ImageFormat::Jpeg, transparent: false, quality: 90,
                               width: None, height: None, crop: None, zoom: 1.0, screen_width: None };
        let out = produce(&raw, &opts).unwrap();
        let decoded = img::load_from_memory(&out).unwrap().to_rgb8();
        let p = decoded.get_pixel(0, 0);
        assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "transparent→white, got {p:?}");
    }
}
