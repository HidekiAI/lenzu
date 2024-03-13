use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use image::DynamicImage;

pub trait OcrTrait {
    // without 'Sized',  won't be able to Box<dyn OcrTrait>
    // i.e.     fn new_ocr(choice: &str) -> Box<dyn crate::ocr_traits::OcrTrait> {...
    fn new() -> Self where Self: Sized; 

    // returns array of Strings of supported languages
    fn init(&self) -> Vec<String>;

    fn evaluate_by_paths(&self, image_path: &str) -> Result<OcrTraitResult, Error>;

    fn evaluate(&self, image: &DynamicImage) -> Result<OcrTraitResult, Error>;
}

pub(crate) struct OcrTraitResult {
    pub text: String,
    pub lines: Vec<String>,
}