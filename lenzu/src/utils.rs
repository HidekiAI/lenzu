use base64::{engine::general_purpose, Engine as _};
use image::{ImageBuffer, ImageFormat, Rgb};
use std::io::Cursor;

const DEBUG_IMAGE_PATH: &str = "/dev/shm/debug_lens.png";

/// Converts raw BGRX/BGRA bytes from X11 to RGB bytes.
pub fn raw_to_rgb(raw: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((raw.len() / 4) * 3);
    for chunk in raw.chunks_exact(4) {
        rgb.push(chunk[2]); // R
        rgb.push(chunk[1]); // G
        rgb.push(chunk[0]); // B
    }
    rgb
}

/// Swaps bytes in place for GDK Pixbuf compatibility (BGRX -> RGBX).
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

pub fn encode_to_base64(rgb_data: &[u8], w: u32, h: u32) -> String {
    let img = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb_data.to_vec()).unwrap();
    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::Png).unwrap();
    general_purpose::STANDARD.encode(buffer.into_inner())
}
