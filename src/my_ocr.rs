extern crate image;
extern crate tesseract;

use image::{GenericImageView, RgbaImage};
use tesseract::Tesseract;

pub struct MyOcr {
    raw_data: Vec<u8>, // raw 32-bits RGBA data (4 bytes per pixel - see BYTES_PER_PIXEL)
    width: u32,
    height: u32,

    is_dirty: bool,           // true if the image has been updated since the last OCR
    pub tesseract: Tesseract, // want private, but because it needs to be mutable, it's public
    image: image::DynamicImage, // 8-bit greyscaled image
}
impl MyOcr {
    pub const OCR_BYTES_PER_PIXEL: u8 = 4;
    pub const OCR_BITS_PER_PIXEL: u8 = 8 * Self::OCR_BYTES_PER_PIXEL; // i.e. 32-bits per pixel
    pub const OCR_FRAME_LANGUAGE: &str = "jpn+jpn_vert+eng"; // For this app, we only want to OCR Japanese text (but will let it recognize English as well)

    // constructor with parameters
    pub fn new(width: u32, height: u32) -> MyOcr {
        println!("TESSDATA_PREFIX: {:?}", std::env::var("TESSDATA_PREFIX"));
        let tesseract = Tesseract::new(None, Some(Self::OCR_FRAME_LANGUAGE));
        let raw_data_rgba32: Vec<u8> = Vec::with_capacity(
            width as usize * Self::OCR_BYTES_PER_PIXEL as usize * height as usize,
        ); // Usually, 4 bytes per pixel (RGBA) for 32-bit image
        match tesseract {
            Ok(t) => {
                // construct image based on data and store just the greyscaled version
                MyOcr {
                    raw_data: raw_data_rgba32.clone(),
                    width: width,
                    height: height,
                    is_dirty: raw_data_rgba32.len() > 0,
                    tesseract: t,
                    image: Self::make_data(width, height, raw_data_rgba32), // as long as it's the last usage of raw_data_rgba32, no need to clone()
                }
            }
            Err(e) => {
                #[cfg(debug_assertions)]
                {
                    // Usually, the cause of this is due to "tessdata" not being found in the current directory
                    // or based on environment variable "TESSDATA_PREFIX" not being set correctly
                    // so print the env-var as well as current directory to help with debugging
                    println!("Current Directory: {:?}", std::env::current_dir());

                    println!("Error: {}", e);
                }
                panic!("Error: {}", e);
            }
        }
    }

    pub fn get_width(&self) -> u32 {
        self.width
    }
    pub fn get_height(&self) -> u32 {
        self.height
    }

    fn make_data(width: u32, height: u32, rgba32: Vec<u8>) -> image::DynamicImage {
        let rgba_image: image::RgbaImage =
            image::ImageBuffer::from_raw(width, height, rgba32).unwrap();
        let gs: image::GrayImage = image::imageops::grayscale(&rgba_image);
        image::DynamicImage::ImageLuma8(gs)
    }

    pub fn set_data(&mut self, rgba32: Vec<u8>, width: u32, height: u32) {
        self.raw_data = rgba32;
        self.width = width;
        self.height = height;
        self.image = Self::make_data(self.width, self.height, self.raw_data.clone());
        self.set_dirty(true);
    }
    pub fn get_text(&mut self) -> String {
        match Tesseract::new(None, Some(Self::OCR_FRAME_LANGUAGE)) {
            Ok(t) => match t.set_image_from_mem(self.image.as_bytes()) {
                Ok(mut slf) => match &slf.get_text() {
                    Ok(text) => {
                        self.tesseract = slf;
                        text.into()
                    }
                    Err(e) => {
                        println!("Error: {}", e);
                        String::new()
                    }
                },
                Err(e) => {
                    println!("Error: {}", e);
                    return String::new();
                }
            },
            Err(e) => {
                println!("Error: {}", e);
                String::new()
            }
        }
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.is_dirty
    }
    pub fn set_dirty(&mut self, is_dirty: bool) {
        self.is_dirty = is_dirty;
    }

    pub(crate) fn draw(&self) -> Result<(), String> {
        // NOTE: we do not reset dirty flag here because we do not have access to rendere surface at this level
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::MyOcr;

    #[test]
    fn test_get_text_with_text() {
        // Create a MyOcr instance with an image containing text
        let width = 32;
        let height = 32;
        let mut my_ocr = MyOcr::new(width, height);

        // Perform OCR and assert the recognized text
        let recognized_text = my_ocr.get_text();
        assert_eq!(recognized_text, "Expected Text");
    }

    #[test]
    fn test_get_text_without_text() {
        // Create a MyOcr instance with an image without text
        let width = 32;
        let height = 32;
        let mut my_ocr = MyOcr::new(width, height);

        // Perform OCR and assert that no text is found
        let recognized_text = my_ocr.get_text();
        assert!(recognized_text.is_empty());
    }
}
