use crate::ocr_traits::OcrTrait;
use anyhow::Error;
use rusty_tesseract::Args;
use windows::Media::Ocr;

// derive from OcrTrait
pub struct OcrTesseract {
    ocr_args: rusty_tesseract::Args,
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

    fn evaluate_by_paths(&self, image_path: &str) -> core::result::Result<String, Error> {
        let img = image::open(image_path).unwrap();
        self.evaluate(&img)
    }

    fn evaluate(&self, image: &image::DynamicImage) -> core::result::Result<String, Error> {
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
            rusty_tesseract::Image::from_dynamic_image(&image); // from_dynamic_image(&gray_scale_image);
        let ocr_result: Result<String, rusty_tesseract::TessError> = match ocr_image {
            Ok(img) => rusty_tesseract::image_to_string(&img, &ocr_args),
            Err(e) => {
                println!("Error: {:?}", e);
                return Err(e.into());
            }
        };
        let total_time = start_ocr.elapsed().as_millis();
        println!("OCR Result ({} mSec): '{:?}'", total_time, ocr_result);
        Ok(ocr_result.unwrap())
    }
}
