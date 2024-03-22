use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use image::{imageops::overlay, DynamicImage, GrayImage, ImageBuffer, RgbImage, Rgba, *};
use imageproc::drawing::{draw_text_mut, text_size};
use rusttype::{Font, Scale};
use rusty_tesseract::image::{GenericImage as _, GenericImageView as _};
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
    io::{Read, Seek},
};

// The BOLD font is about 32x32 pixels
const DEFAULT_FONT_SIZE: f32 = 32.0;

// fonts as constant (data pool)
const FONT_DATA: &[u8] = if cfg!(target_os = "windows") {
    include_bytes!("..\\..\\assets\\fonts\\Noto_Sans_JP\\static\\NotoSansJP-Regular.ttf")
} else {
    include_bytes!("../../assets/fonts/Noto_Sans_JP/static/NotoSansJP-Regular.ttf")
};

const FONT_DATA_BOLD: &[u8] = if cfg!(target_os = "windows") {
    include_bytes!("..\\..\\assets\\fonts\\Noto_Sans_JP\\static\\NotoSansJP-Bold.ttf")
} else {
    include_bytes!("../../assets/fonts/Noto_Sans_JP/static/NotoSansJP-Bold.ttf")
};

#[derive(Debug, Clone)]
pub struct OCRImage {
    image_path: String, // canonicalized path to the image (differs on format based on platform, use std::fs::canonicalize() to get it)
    dynamic_image: Option<DynamicImage>, // could be BMP, Png, Jpeg, etc.
    ttf_font: ab_glyph::FontArc, // use include_bytes!("path/to/font.ttf") to load font
    ttf_font_bold: ab_glyph::FontArc, // use include_bytes!("path/to/font.ttf") to load font
}

impl Display for OCRImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OCRImage: {} ({}x{})",
            self.image_path,
            self.dynamic_image
                .as_ref()
                .map(|img| img.width().to_string())
                .unwrap_or("0".to_string()),
            self.dynamic_image
                .as_ref()
                .map(|img| img.height().to_string())
                .unwrap_or("0".to_string())
        )
    }
}

impl From<image::ImageBuffer<Rgba<u8>, Vec<u8>>> for OCRImage {
    fn from(img: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Self {
        OCRImage {
            dynamic_image: Some(DynamicImage::from(img)),
            // initialize other fields as needed
            ..OCRImage::new(None)
        }
    }
}

pub struct RawBitmapImage {
    pub image: Vec<u8>, // due to lifetime concern, cannot use &[u8] here
    pub width: u32,
    pub height: u32,
}
impl From<RawBitmapImage> for OCRImage {
    fn from(from_image: RawBitmapImage) -> Self {
        let load_result = image::load_from_memory(from_image.image.as_slice());
        let img = match load_result {
            Ok(loaded_image) => loaded_image,
            Err(e) => {
                println!("Error loading image from memory: {}, attempting to dynamically construct rgba8...", e);
                let mut rebuilt_image =
                    image::DynamicImage::new_rgba8(from_image.width, from_image.height);
                // now update image with buffer data as if the data is a PNG (order by height (row-ordered))
                for y in 0..from_image.height {
                    for x in 0..from_image.width {
                        let index = (y * from_image.width + x) as usize * 4;
                        let r = from_image.image[index];
                        let g = from_image.image[index + 1];
                        let b = from_image.image[index + 2];
                        let a = from_image.image[index + 3];
                        let pixel = image::Rgba([r, g, b, a]);
                        rebuilt_image.put_pixel(x, y, pixel);
                    }
                }
                rebuilt_image
            }
        };
        println!(
            "from_raw_bytes() - Image is {} bytes, ColorType: {:?},  Dimensions: {:?}",
            from_image.image.len(),
            img.color(),
            img.dimensions()
        );
        OCRImage {
            dynamic_image: Some(img),
            // initialize other fields as needed
            ..OCRImage::new(None)
        }
    }
}

pub struct RawPngImage {
    pub image: Vec<u8>, // due to lifetime concern, cannot use &[u8] here
    pub width: u32,
    pub height: u32,
}
impl From<RawPngImage> for OCRImage {
    fn from(value: RawPngImage) -> Self {
        let load_result =
            image::load_from_memory_with_format(value.image.as_slice(), image::ImageFormat::Png);
        let img = match load_result {
            Ok(loaded_image) => loaded_image,
            Err(e) => {
                println!("Error loading image from memory: {}, attempting to dynamically construct rgba8...", e);
                let mut rebuilt_image = image::DynamicImage::new_rgba8(value.width, value.height);
                // now update image with buffer data as if the data is a PNG (order by height (row-ordered))
                for y in 0..value.height {
                    for x in 0..value.width {
                        let index = (y * value.width + x) as usize * 4;
                        let r = value.image[index];
                        let g = value.image[index + 1];
                        let b = value.image[index + 2];
                        let a = value.image[index + 3];
                        let pixel = image::Rgba([r, g, b, a]);
                        rebuilt_image.put_pixel(x, y, pixel);
                    }
                }
                // now convert this to ImageFormat::Png
                let buffer =
                    Vec::with_capacity(value.width as usize * value.height as usize * 4 + 1024);
                let mut buf = std::io::Cursor::new(buffer);
                rebuilt_image
                    .write_to(&mut buf, image::ImageFormat::Png)
                    .expect("Error writing image to buffer as PNG");

                buf.set_position(0);
                // convert buffer to DynamicImage
                let ret =
                    image::load_from_memory_with_format(buf.get_ref(), image::ImageFormat::Png)
                        .expect("Error loading image from memory");
                ret
            }
        };
        println!(
            "from_png_bytes() - Image is {} bytes, ColorType: {:?},  Dimensions: {:?}",
            value.image.len(),
            img.color(),
            img.dimensions()
        );
        OCRImage {
            dynamic_image: Some(img),
            // initialize other fields as needed
            ..OCRImage::new(None)
        }
    }
}

impl From<image::ImageBuffer<image::LumaA<u8>, Vec<u8>>> for OCRImage {
    fn from(img: image::ImageBuffer<image::LumaA<u8>, Vec<u8>>) -> Self {
        OCRImage {
            dynamic_image: Some(DynamicImage::from(img)),
            // initialize other fields as needed
            ..OCRImage::new(None)
        }
    }
}

impl From<image::DynamicImage> for OCRImage {
    fn from(img: image::DynamicImage) -> Self {
        OCRImage {
            dynamic_image: Some(img),
            // initialize other fields as needed
            ..OCRImage::new(None)
        }
    }
}
// as of current, imageproc version if DynamicImage is the same as image::DynamicImage
//impl From<imageproc::image::DynamicImage> for OCRImage {
//    fn from(img: imageproc::image::DynamicImage) -> Self {
//        let mut buffer = Vec::with_capacity(img.as_bytes().len());
//        img.as_bytes()
//            .read_to_end(&mut buffer)
//            .expect("Error reading image bytes");
//        let img =
//            image::load_from_memory(buffer.as_slice()).expect("Error loading image from memory");
//        OCRImage {
//            dynamic_image: Some(img),
//            // initialize other fields as needed
//            ..OCRImage::new(None)
//        }
//    }
//}

impl From<rusty_tesseract::image::DynamicImage> for OCRImage {
    fn from(img: rusty_tesseract::image::DynamicImage) -> Self {
        let mut buffer = Vec::with_capacity(img.as_bytes().len());
        img.as_bytes()
            .read_to_end(&mut buffer)
            .expect("Error reading image bytes");
        let img =
            image::load_from_memory(buffer.as_slice()).expect("Error loading image from memory");
        OCRImage {
            dynamic_image: Some(img),
            // initialize other fields as needed
            ..OCRImage::new(None)
        }
    }
}

impl OCRImage {
    pub fn get_image_path(&self) -> &str {
        &self.image_path
    }
    pub fn get_dynamic_image(&self) -> &DynamicImage {
        self.dynamic_image.as_ref().unwrap()
    }
    pub fn get_font(&self) -> &ab_glyph::FontArc {
        &self.ttf_font
    }
    pub fn get_font_bold(&self) -> &ab_glyph::FontArc {
        &self.ttf_font_bold
    }

    pub fn new(possible_path_to_image: Option<&str>) -> OCRImage {
        let font: ab_glyph::FontArc = ab_glyph::FontArc::try_from_slice(FONT_DATA)
            .expect("Failed to load font 'NotoSansJP-Regular.ttf' from memory");

        let font_bold: ab_glyph::FontArc = ab_glyph::FontArc::try_from_slice(FONT_DATA_BOLD)
            .expect("Failed to load font 'NotoSansJP-Bold.ttf' from memory");

        // load image from file-paths
        let img = match possible_path_to_image {
            Some(path) => {
                let img = image::open(path)
                    .expect(format!("Error loading image file '{}'", path).as_str());
                Some(img)
            }
            None => None,
        };

        OCRImage {
            image_path: possible_path_to_image.unwrap_or("").to_string(),
            dynamic_image: img,
            ttf_font: font,
            ttf_font_bold: font_bold,
        }
    }

    pub fn load_image(&mut self, path: &str) -> Result<(), Error> {
        let img = image::open(path).expect(format!("Error loading image file '{}'", path).as_str());
        self.dynamic_image = Some(img);
        Ok(())
    }

    pub(crate) fn is_png(&self) -> bool {
        match &self.dynamic_image {
            Some(img) => Self::is_valid_png(img.as_bytes()),
            None => false,
        }
    }
    // some PNG's are not kosher...
    pub fn is_valid_png(image: &[u8]) -> bool {
        let guess: imageproc::image::ImageFormat = match imageproc::image::guess_format(image) {
            Ok(format) => format,
            Err(_) => return false,
        };

        // PNG signature: Check the PNG Header: The first 8 bytes of a valid PNG file should be the
        // following hexadecimal values: 89 50 4E 47 0D 0A 1A 0A.
        // These represent the PNG magic number. Verify that your &[u8] array starts with these bytes.
        let png_signature: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
        let has_png_signature = image.len() >= 8 && image[0..8] == png_signature;

        // PNG IHDR
        let has_png_ihdr = image.len() >= 16 && &image[12..16] == b"IHDR";

        // PNG IEND
        let has_png_iend =
            image.len() >= 12 && &image[image.len() - 12..] == b"IEND\xae\x42\x60\x82";

        has_png_signature && image.len() > 8 || (guess == imageproc::image::ImageFormat::Png)
    }

    // because there seems to be mismatch on the types of images, we need to convert the image to the
    pub fn to_imageproc_dynamic_image(
        image: &[u8],
        width: u32,
        height: u32,
    ) -> imageproc::image::DynamicImage {
        // if the buffer is just raw buffer rather than PNG buffer, we need to load it differently
        if Self::is_valid_png(image) {
            let transformed_image = imageproc::image::load_from_memory(image).expect(
                format!(
                    "\nUnable to load image from memory ({} bytes)!",
                    image.len()
                )
                .as_str(),
            );
            println!(
            "to_imageproc_dynamic_image() - Image is {} bytes, ColorType: {:?},  Dimensions: {:?}",
            image.len(),
            transformed_image.color(),
            transformed_image.dimensions()
        );
            transformed_image
        } else {
            // create a blank image
            let mut img = imageproc::image::DynamicImage::new_rgba8(width, height);
            // now update image with buffer data as if the data is a PNG (order by height (row-ordered))
            for y in 0..height {
                for x in 0..width {
                    let index = (y * width + x) as usize * 4;
                    let r = image[index];
                    let g = image[index + 1];
                    let b = image[index + 2];
                    let a = image[index + 3];
                    img.put_pixel(x, y, imageproc::image::Rgba([r, g, b, a]));
                }
            }
            img
        }
    }

    pub fn to_rusty_tesseract_dynamic_image(
        image: &[u8],
        width: u32,
        height: u32,
    ) -> rusty_tesseract::image::DynamicImage {
        if Self::is_valid_png(image) {
            let transformed_image = rusty_tesseract::image::load_from_memory(image).expect(
                format!(
                    "\nUnable to load image from memory ({} bytes)!",
                    image.len()
                )
                .as_str(),
            );
            println!(
        "to_rusty_tesseract_dynamic_image() - Image is {} bytes, ColorType: {:?},  Dimensions: {:?}",
        image.len(),
        transformed_image.color(),
        transformed_image.dimensions()
    );
            transformed_image
        } else {
            // create a blank image
            let mut img = rusty_tesseract::image::DynamicImage::new_rgba8(width, height);
            // now update image with buffer data as if the data is a PNG (order by height (row-ordered))
            for y in 0..height {
                for x in 0..width {
                    let index = (y * width + x) as usize * 4;
                    let r = image[index];
                    let g = image[index + 1];
                    let b = image[index + 2];
                    let a = image[index + 3];
                    let pixel = rusty_tesseract::image::Rgba([r, g, b, a]);
                    img.put_pixel(x, y, pixel);
                }
            }
            img
        }
    }

    // convert from raw byte-array to DynamicImage
    pub fn from_raw_bytes(&mut self, raw_image: RawBitmapImage) -> DynamicImage {
        let x = OCRImage::from(raw_image);
        self.dynamic_image = x.dynamic_image;
        self.dynamic_image.clone().unwrap()
    }

    // convert from byte-array to DynamicImage of assumed type
    pub fn from_png_bytes(&mut self, image: RawPngImage) -> DynamicImage {
        let my_image = OCRImage::from(image);
        self.dynamic_image = my_image.dynamic_image;
        self.dynamic_image.clone().unwrap()
    }

    // saves the dynamic image to a file (not the font)
    pub fn save(&self, path: &str) {
        if let Some(img) = &self.dynamic_image {
            img.save(path)
                .expect(format!("Error saving image to '{}'", path).as_str());
        }
    }

    pub fn set_image(&mut self, img: DynamicImage) {
        self.dynamic_image = Some(img);
    }

    pub fn get_image(&self) -> &DynamicImage {
        match &self.dynamic_image {
            Some(img) => img,
            None => {
                panic!("get_image(): No image loaded");
            }
        }
    }

    pub fn get_possible_image(&self) -> Option<&DynamicImage> {
        self.dynamic_image.as_ref()
    }

    pub fn get_image_bytes(&self) -> &[u8] {
        match &self.dynamic_image {
            Some(img) => {
                img.as_bytes()
            }
            None => {
                // return empty array
                &[]
            }
        }
    }

    pub fn overlay_text(
        &self,
        text: &str,
        textx: i32,
        texty: i32,
        //c_background_image: &DynamicImage,
    ) -> DynamicImage {
        let mut background_image = self.get_image().clone();
        if text.is_empty() {
            println!("Warning: No text to overlay onto image");
            return background_image; // return back the original cloned (for optimization, make sure to pretest text length before calling here, so we won't even need to clone here)
        }

        let scale = Scale::uniform(DEFAULT_FONT_SIZE);
        println!("overlay_text() - Scale: {:?}", scale);

        // first, we need to determine how wide the text is, and if it is wider than the image,
        // we need to break the text down to multiple lines
        let image_char_width =
            std::cmp::max((background_image.width() / scale.x as u32) - 10u32, 1);
        let mut text_width: u32 = text.len() as u32; // number of characters
        let mut text_height: u32 = 1; // number of lines
        let mut text_width_pixels: u32 = text_width * scale.x as u32;
        let mut text_height_pixels: u32 = text_height * scale.y as u32;
        if text_width > image_char_width {
            // if the text is wider than the image, then we need to break it down to multiple lines
            text_width = image_char_width;
            text_height = (text.len() as u32 / text_width) + 1;
            text_width_pixels = text_width * scale.x as u32;
            text_height_pixels = text_height * scale.y as u32;
        }
        // iterate each char and add newline when we reach text_width
        let binding = text
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if (i as u32 + 1) % text_width == 0 {
                    format!("{}\n", c)
                } else {
                    format!("{}", c)
                }
            })
            .collect::<String>();
        let multi_lined_text: Vec<&str> = binding.split("\n").collect();

        // Note that imageproc::overlay() only works on same byte depth (i.e. 8-bit, 16-bit, 24-bit, 32-bit, etc.),
        let mut color_type: image::ColorType = background_image.color();

        // see if we can transform the background_image to  Rgba<u8> IF it is just plain gray scale or other format other than 32-bits
        // if this is not possible, we'll always just convert to Luma<u8>  (lowest comman denominator) but I'd like to have it as 32-bits
        // so that I can have the text in RED...
        if color_type != image::ColorType::Rgba8 {
            // convert to Rgba<u8>
            let (img_width, img_height) = background_image.dimensions().clone();
            let binding = background_image.into_rgba8().clone();
            let raw_png_image = RawPngImage {
                image: binding.as_raw().to_vec(),
                width: img_width,
                height: img_height,
            };
            if Self::is_valid_png(&raw_png_image.image) {
                println!("overlay_text() - Image is PNG");
            } else {
                println!("overlay_text() - Image is NOT PNG");
            }
            let rgba_image = OCRImage::from(raw_png_image);
            let rgba_image_buffer =  // : ImageBuffer<Rgba<u8>, Vec<u8>> =
            rgba_image.get_image().as_rgba8().unwrap();
            // now convert it back to DynamicImge
            let ret = DynamicImage::ImageRgba8(rgba_image_buffer.clone());
            background_image = ret;
            color_type = background_image.color();
        }
        println!("overlay_text() - ColorType: {:?}", color_type);

        // instantiate ImageBuffer matching same type as background_image via creating a sample pixel matching color_type
        let p = image::Rgba([0u8, 0, 0, 0]);
        let mut text_image_canvas =
            image::ImageBuffer::from_pixel(text_width_pixels, text_height_pixels, p);

        // Load a font (you can replace this with your own font)
        // Create a font (Meiryo or any other suitable Japanese font)
        //let font_data: &[u8] = if cfg!(target_os = "windows") {
        //    include_bytes!("..\\..\\assets\\fonts\\kochi-mincho-subst.ttf")
        //} else {
        //    include_bytes!("../../assets/fonts/kochi-mincho-subst.ttf")
        //};
        ////let font: Font<'_> = rusttype::Font::try_from_bytes(font_data).expect("Failed to load font");
        //let font: ab_glyph::FontArc = ab_glyph::FontArc::try_from_slice(font_data)
        //    .expect("Failed to load font 'kochi-mincho-subst.ttf'");
        //let scale = Scale {
        //    x: image.width() as f32 * 0.2,
        //    y: font.scale_for_pixel_height(DEFAULT_FONT_SIZE),
        //};
        //let fscale = font.scale_for_pixel_height(DEFAULT_FONT_SIZE);

        // render text on the text_image_canvas
        println!(
            "Width: {} (WinX:{}, pixels:{})\nHeight: {} (WinY:{}, pixels:{})\n{:?}\n\n",
            text_width, 
            background_image.width(),
            text_height_pixels,
            text_height, 
            background_image.height(),
            text_width_pixels,
            multi_lined_text,
        );

        // Draw each line
        let mut line_y: u32 = 0;
        for (index, line) in multi_lined_text.iter().enumerate() {
            println!(
                "Line {}: Y={} - '{}' ({} chars)",
                index,
                line_y,
                line,
                line.len()
            );
            draw_text_mut(
                &mut text_image_canvas,                // canvas surface
                image::Rgba([0xff, 0x40, 0x40, 0xff]), // font color
                0,
                line_y as i32, // Q: Do we need to shift pixels down?
                scale.y,
                &self.ttf_font_bold,
                &line, // text to render (will come out as blank if the UTF8 is not supported by ttf)
            );
            let (ts_width, ts_height) = text_size(scale.y, &self.ttf_font_bold, line); // Adjust y position for the next line
            line_y += ts_height;
        }

        // overlay the two images.  Bottom (background) image is the original image, and the top (foreground) image is the text
        let mut overlayed_image: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> = background_image
            .as_rgba8()
            .expect("Unable to convert bg-image as rgba8")
            .clone();
        let overlay_x: i64 = textx as i64 + scale.x as i64;
        let overlay_y: i64 = texty as i64 + scale.y as i64;
        overlay(
            &mut overlayed_image,
            &mut text_image_canvas,
            overlay_x,
            overlay_y,
        );

        // and finally, convert it back to DynamicImage
        let ret = DynamicImage::ImageRgba8(overlayed_image);
        println!(
            "Overlayed text onto image ({}x{}) - {} chars",
            ret.width(),
            ret.height(),
            text.len()
        );
        //if cfg!(debug_assertions) {
        //    // append first 10 characters of the text to the filename ( replace space with no-space)
        //    let first_10 = &text[0..10]
        //        .replace(" ", "")
        //        .replace(":", "_")
        //        .replace("?", "_")
        //        .replace("\\", "_")
        //        .replace("/", "_");
        //    let filename = format!("overlay_text_{}.png", first_10);
        //    println!("Saving: {}", filename.clone());
        //    match text_image_canvas.save(filename.clone()) {
        //        Ok(_) => println!(
        //            "Saved: {} - {} bytes",
        //            filename.clone(),
        //            text_image_canvas.len()
        //        ),
        //        Err(e) => println!("Error: Could not save {} - {}", filename.clone(), e),
        //    }
        //    let filename = format!("overlayed_text_{}.png", first_10);
        //    match ret.clone().save(filename.clone()) {
        //        Ok(_) => println!(
        //            "Saved: {} - {} bytes",
        //            filename.clone(),
        //            ret.clone().as_bytes().len()
        //        ),
        //        Err(e) => println!("Error: Could not save {} - {}", filename.clone(), e),
        //    }
        //}

        ret
    }
}
