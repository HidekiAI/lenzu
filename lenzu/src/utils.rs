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

pub fn save_debug_image(rgb_data: &[u8], w: u32, h: u32) {
    if let Some(img) = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb_data.to_vec()) {
        let _ = img.save(DEBUG_IMAGE_PATH);
    }
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
}
