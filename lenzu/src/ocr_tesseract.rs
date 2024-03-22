use crate::ocr_traits::{self, OcrRect, OcrTrait, OcrTraitResult};
use anyhow::Error;
use rusty_tesseract::image::GenericImageView;
use rusty_tesseract::Args;

// derive from OcrTrait
pub struct OcrTesseract {
    ocr_args: rusty_tesseract::Args,
}

impl OcrTesseract {
    pub fn new_with_args(ocr_args: rusty_tesseract::Args) -> Self {
        OcrTesseract { ocr_args }
    }

    //fn to_ocr_rect(rect: ) -> OcrRect {
    //}

    //fn to_ocr_word(line_index: u16, word: ) -> ocr_traits::OcrWord {
    //}

    //fn to_ocr_line(
    //    line_index: u16,
    //    words:
    //) -> ocr_traits::OcrLine {
    //}

    //fn to_ocr_lines(
    //    lines:
    //) -> Vec<ocr_traits::OcrLine> {
    //}
    fn to_ocr_lines(lines: Vec<String>) -> Vec<ocr_traits::OcrLine> {
        lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let words = line
                    .split_whitespace()
                    .map(|word| {
                        let rect = OcrRect::new(0, 0, 8, 8);
                        ocr_traits::OcrWord::new(word.to_string(), i as u16, rect)
                    })
                    .collect();
                ocr_traits::OcrLine::new(words)
            })
            .collect()
    }
}

impl OcrTrait for OcrTesseract {
    fn new() -> Self
    where
        Self: Sized,
    {
        OcrTesseract {
            ocr_args: Args {
                lang: "jpn+jp_vert+osd".into(),
                psm: Some(5), // the best we can do that is closest on jpn_vert
                ..rusty_tesseract::Args::default()
            },
        }
    }

    fn init(&self) -> Vec<String> {
        //tesseract version
        let tesseract_version = rusty_tesseract::get_tesseract_version().unwrap();
        println!("Tesseract - Version is: {:?}", tesseract_version);

        //available languages
        let tesseract_langs = rusty_tesseract::get_tesseract_langs().unwrap();
        println!(
            "Tesseract - The available languages are: {:?}",
            tesseract_langs
        );

        //available config parameters
        let parameters = rusty_tesseract::get_tesseract_config_parameters().unwrap();
        println!(
            "Tesseract - Config parameter: {}",
            parameters.config_parameters.first().unwrap()
        );

        tesseract_langs
    }

    fn evaluate_by_paths(
        &self,
        image_path: &str,
    ) -> core::result::Result<ocr_traits::OcrTraitResult, Error> {
        let img = rusty_tesseract::image::open(image_path).unwrap();
        self.evaluate(&ocr_traits::to_imageproc_dynamic_image(
            img.as_bytes(),
            img.width(),
            img.height(),
        ))
    }

    fn evaluate(
        &self,
        image: &imageproc::image::DynamicImage,
    ) -> core::result::Result<ocr_traits::OcrTraitResult, Error> {
        let rusty_image = &ocr_traits::to_rusty_tesseract_dynamic_image(
            image.as_bytes(),
            image.width(),
            image.height(),
        );
        let supported_lang = rusty_tesseract::get_tesseract_langs().unwrap().join("+");
        // Default OEM=3 (based on what is available)
        // For Manga, PSM should be 6 in gener
        let ocr_args: rusty_tesseract::Args = Args {
            lang: supported_lang.into(),
            //..Default::default()
            ..self.ocr_args.clone()
        };

        let start_ocr = std::time::Instant::now();
        let ocr_image: Result<rusty_tesseract::Image, rusty_tesseract::TessError> =
            rusty_tesseract::Image::from_dynamic_image(rusty_image); // from_dynamic_image(&gray_scale_image);
        let ocr_result: Result<String, rusty_tesseract::TessError> = match ocr_image {
            Ok(img) => rusty_tesseract::image_to_string(&img, &ocr_args),
            Err(e) => {
                println!("Error: {:?}", e);
                return Err(e.into());
            }
        };
        let total_time = start_ocr.elapsed().as_millis();
        println!("OCR Result ({} mSec): '{:?}'", total_time, ocr_result);
        let ocr_str = ocr_result.unwrap();
        let x_min = 0;
        let y_min = 0;
        let lines = vec![ocr_str.split("\n").collect()];
        let rect_lines: Vec<ocr_traits::OcrLine> = Self::to_ocr_lines(lines.clone());
        let result = OcrTraitResult {
            text: ocr_str.clone(),
            lines: lines.clone(),
            rects: rect_lines,
        };
        Ok(result)
    }
}
