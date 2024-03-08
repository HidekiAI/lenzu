use image::DynamicImage;
use anyhow::Error;  // the most easiest way to handle errors
use core::result::Result;

pub trait OcrTrait {
    fn new() -> Self;
    // returns array of Strings of supported languages
    fn init(&self) -> Vec<String>;
    fn evaluate_by_paths(&self, image_path: &str) -> Result<String, Error>;
    fn evaluate(&self, image: &DynamicImage) -> Result<String, Error>;
}