use crate::ocr_traits::OcrTrait;
use rusty_tesseract::Args;

pub struct OcrTesseract {
    ocr_args: rusty_tesseract::Args,
}

impl OcrTrait for OcrTesseract {
    fn new() -> Self {
        OcrTesseract {
            ocr_args: rusty_tesseract::Args {
                lang: "jpn+jp_vert+osd".into(),
                psm: Some(5), // the best we can do that is closest on jpn_vert
                ..rusty_tesseract::Args::default()
            },
        }
    }

    fn init(&self) {
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
    }

    fn evaluate_by_paths(&self, image_path: &str) -> String {
        let img = image::open(image_path).unwrap();
        self.evaluate(&img)
    }

    fn evaluate(&self, image: &image::DynamicImage) -> String {
        todo!("CODE ME!")
    }
}
