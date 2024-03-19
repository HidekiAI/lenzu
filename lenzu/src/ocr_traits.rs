use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use image::DynamicImage;
use std::collections::HashMap;

pub trait OcrTrait {
    // without 'Sized',  won't be able to Box<dyn OcrTrait>
    // i.e.     fn new_ocr(choice: &str) -> Box<dyn crate::ocr_traits::OcrTrait> {...
    fn new() -> Self
    where
        Self: Sized;

    // returns array of Strings of supported languages
    fn init(&self) -> Vec<String>;

    fn evaluate_by_paths(&self, image_path: &str) -> Result<OcrTraitResult, Error>;

    fn evaluate(&self, image: &DynamicImage) -> Result<OcrTraitResult, Error>;
}

//
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OcrRect {
    // NOTE: Because we do not know whether the coordinates are based on text poistion or pixel position,
    // we have to deal with it in signed-integer because if it is in pixels, it can be negative  based on
    // where the origin is...
    pub x_min: i32, // upper left corner of the rectangle
    pub y_min: i32,
    pub x_max: i32, // lower right corner of the rectangle
    pub y_max: i32,
}
impl OcrRect {
    pub fn new(x_min: i32, y_min: i32, x_max: i32, y_max: i32) -> Self {
        OcrRect {
            x_min,
            y_min,
            x_max,
            y_max,
        }
    }
    pub fn from(x_min: i32, y_min: i32, width: u32, height: u32) -> Self {
        OcrRect {
            x_min,
            y_min,
            x_max: x_min + width as i32,
            y_max: y_min + height as i32,
        }
    }
    pub fn width(&self) -> u32 {
        (self.x_max - self.x_min) as u32 // TODO: how do we garaunteed that we will not have a negative width?
    }
    pub fn height(&self) -> u32 {
        (self.y_max - self.y_min) as u32 // TODO: make sure we do not have a negative height!
    }
}

pub(crate) struct OcrTraitResult {
    pub lines: Vec<String>, // each line of text, sequentially ordered (up to OCR whether it is horizontal:left-to-right, or vertical:top-to-bottom-left-to-right )
    pub text: String,       // entier text split via newlines (built from lines)
    pub rects: HashMap<OcrRect, Vec<String>>, // for each (rectangle) block of text (see lines)
}

impl OcrTraitResult {
    //pub fn new() -> Self { }  // NOTE: There will be no default constructor because we want to make sure that we have the necessary data
}
