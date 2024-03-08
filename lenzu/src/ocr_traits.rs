use image::DynamicImage;

pub trait OcrTrait {
    fn new() -> Self;
    fn init(&self);
    fn evaluate_by_paths(&self, image_path: &str) -> String;
    fn evaluate(&self, image: &DynamicImage) -> String;
}