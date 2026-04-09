use base64::{engine::general_purpose, Engine as _};
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageBuffer, ImageFormat, Rgb};
use std::io::Cursor;

const DEBUG_IMAGE_PATH: &str = "/dev/shm/lenzu/debug_lens.png";

pub fn raw_to_rgb(raw: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((raw.len() / 4) * 3);
    for chunk in raw.chunks_exact(4) {
        rgb.push(chunk[2]); // R
        rgb.push(chunk[1]); // G
        rgb.push(chunk[0]); // B
    }
    rgb
}

/// Convert raw BGRX capture bytes into a `DynamicImage` for further processing.
pub fn raw_to_dynamic_image(raw: &[u8], w: u32, h: u32) -> DynamicImage {
    let rgb = raw_to_rgb(raw);
    let buf = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb)
        .expect("raw_to_dynamic_image: dimensions mismatch");
    DynamicImage::ImageRgb8(buf)
}

pub fn swap_bytes_for_pixbuf(raw: &mut [u8]) {
    for chunk in raw.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
}

/// Save the exact image that will be sent over the wire: greyscaled, same dimensions as the
/// crop/lens capture.  Written just before the OCR/LLM call so the file reflects the actual
/// payload rather than the raw RGB lens capture.
pub fn save_prewire_debug(image: &DynamicImage) {
    let gray = image.grayscale();
    let _ = gray.save(DEBUG_IMAGE_PATH);
}

/// Encode as **grayscale** PNG → base64.
/// Applied to ALL captures (primary and fallback) — OCR only needs luminance.
pub fn encode_as_grayscale(image: &DynamicImage) -> String {
    let gray = image.grayscale();
    let mut buf = Cursor::new(Vec::new());
    gray.write_to(&mut buf, ImageFormat::Png).unwrap();
    general_purpose::STANDARD.encode(buf.into_inner())
}

/// Encode as **grayscale + proportionally downscaled** PNG → base64.
/// `max_dim`: longest-edge pixel cap; 0 = no downscale.
/// Applied only before remote (fallback) calls to reduce token cost.
pub fn encode_for_fallback(image: &DynamicImage, max_dim: u32) -> String {
    let gray = image.grayscale();
    let scaled = if max_dim > 0 {
        let (w, h) = gray.dimensions();
        if w > max_dim || h > max_dim {
            gray.resize(max_dim, max_dim, FilterType::Lanczos3)
        } else {
            gray
        }
    } else {
        gray
    };
    let mut buf = Cursor::new(Vec::new());
    scaled.write_to(&mut buf, ImageFormat::Png).unwrap();
    general_purpose::STANDARD.encode(buf.into_inner())
}

/// Legacy helper kept for callers that already have a raw RGB vec.
pub fn encode_to_base64(rgb_data: &[u8], w: u32, h: u32) -> String {
    let img = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb_data.to_vec()).unwrap();
    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::Png).unwrap();
    general_purpose::STANDARD.encode(buffer.into_inner())
}

/// Save debug files for Ctrl+Shift+Click fullscreen scan.
///
/// Writes two files to `/dev/shm/lenzu/`:
/// - `fullscreen-debug.png` — fullscreen capture with every detected bounding box
///   drawn as a colour-coded hollow rectangle (2-pixel border, cycling through 6 colours).
/// - `fullscreen-debug.json` — JSON object `{ count, boxes: [{idx,x1,y1,x2,y2,width,height}] }`.
///
/// Called unconditionally after `TextDetector::detect()` so an empty `boxes` slice still
/// produces the files (zero-box JSON, undecorated PNG) — useful for confirming that DBNet
/// ran but found nothing.
pub fn save_fullscreen_debug(
    image: &DynamicImage,
    boxes: &[crate::ocr::text_detection::TextBoundingBox],
) {
    use image::Rgb;

    let mut rgb = image.to_rgb8();
    let (img_w, img_h) = rgb.dimensions();

    let palette: &[Rgb<u8>] = &[
        Rgb([255, 0, 0]),    // red
        Rgb([0, 220, 0]),    // green
        Rgb([0, 120, 255]),  // blue
        Rgb([255, 200, 0]),  // yellow
        Rgb([255, 0, 255]),  // magenta
        Rgb([0, 220, 220]),  // cyan
    ];

    for (i, b) in boxes.iter().enumerate() {
        let color = palette[i % palette.len()];
        let x1 = b.x1.min(img_w.saturating_sub(1));
        let y1 = b.y1.min(img_h.saturating_sub(1));
        let x2 = b.x2.min(img_w.saturating_sub(1));
        let y2 = b.y2.min(img_h.saturating_sub(1));

        // 2-pixel thick hollow rectangle
        for t in 0u32..2 {
            let lx = x1.saturating_sub(t);
            let ly = y1.saturating_sub(t);
            let rx = (x2 + t).min(img_w - 1);
            let ry = (y2 + t).min(img_h - 1);
            for x in lx..=rx {
                rgb.put_pixel(x, ly, color);
                rgb.put_pixel(x, ry, color);
            }
            for y in ly..=ry {
                rgb.put_pixel(lx, y, color);
                rgb.put_pixel(rx, y, color);
            }
        }
    }

    let _ = rgb.save("/dev/shm/lenzu/fullscreen-debug.png");

    let json_boxes: Vec<serde_json::Value> = boxes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            serde_json::json!({
                "idx": i,
                "x1": b.x1,
                "y1": b.y1,
                "x2": b.x2,
                "y2": b.y2,
                "width": b.x2.saturating_sub(b.x1),
                "height": b.y2.saturating_sub(b.y1),
            })
        })
        .collect();

    if let Ok(s) = serde_json::to_string_pretty(&serde_json::json!({
        "count": boxes.len(),
        "boxes": json_boxes,
    })) {
        let _ = std::fs::write("/dev/shm/lenzu/fullscreen-debug.json", s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_to_rgb_conversion() {
        // Mock 1 pixel: B=10, G=20, R=30, X=0
        let raw = vec![10, 20, 30, 0];
        let rgb = raw_to_rgb(&raw);
        assert_eq!(rgb, vec![30, 20, 10]); // Should be RGB
    }

    #[test]
    fn test_pixbuf_byte_swap() {
        // Start with BGRX: 10, 20, 30, 255
        let mut data = vec![10, 20, 30, 255];
        swap_bytes_for_pixbuf(&mut data);
        // Should become RGBX: 30, 20, 10, 255
        assert_eq!(data, vec![30, 20, 10, 255]);
    }

    #[test]
    fn test_base64_is_not_empty() {
        let rgb = vec![255, 255, 255]; // 1 white pixel
        let b64 = encode_to_base64(&rgb, 1, 1);
        assert!(!b64.is_empty());
    }

    // ── OPT-2 tests: encode_for_fallback downscaling behaviour ───────────────

    fn rgb_image(w: u32, h: u32) -> DynamicImage {
        use image::{ImageBuffer, Rgb};
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |_, _| Rgb([128u8, 64, 32])))
    }

    fn decoded_dims(b64: &str) -> (u32, u32) {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        image::load_from_memory(&bytes).unwrap().dimensions()
    }

    #[test]
    fn encode_for_fallback_caps_longest_edge() {
        // 600×400 image, cap 512 → longest edge becomes 512, short edge scales proportionally
        let (w, h) = decoded_dims(&encode_for_fallback(&rgb_image(600, 400), 512));
        assert_eq!(w, 512, "longest edge should be capped at 512");
        assert!(h < 512 && h > 0, "short edge should be < 512 and > 0, got {h}");
    }

    #[test]
    fn encode_for_fallback_zero_is_noop() {
        // max_dim = 0 must leave dimensions unchanged (no downscale)
        let (w, h) = decoded_dims(&encode_for_fallback(&rgb_image(600, 400), 0));
        assert_eq!(w, 600);
        assert_eq!(h, 400);
    }

    #[test]
    fn encode_for_fallback_no_upscale_when_under_limit() {
        // Image already smaller than the cap — must not be enlarged
        let (w, h) = decoded_dims(&encode_for_fallback(&rgb_image(300, 200), 512));
        assert_eq!(w, 300);
        assert_eq!(h, 200);
    }
}
