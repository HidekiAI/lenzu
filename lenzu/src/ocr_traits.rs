use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use image::DynamicImage;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

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

#[derive(Debug)]
pub(crate) struct OcrTraitResult {
    pub text: String,       // entier text split via newlines (built from lines)
    pub lines: Vec<String>, // each line of text (collection of words), sequentially ordered (up to OCR whether it is horizontal:left-to-right, or vertical:top-to-bottom-left-to-right )
    pub rects: HashMap<OcrRect, Vec<String>>, // for each (rectangle) block of text (collection of words, see lines)
}

impl Display for OcrTraitResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let str_lines = self.lines.join("\n");
        // join all words into single  line, and split  each rects into newlines
        let str_rects = self
            .rects
            .iter()
            .fold("".to_string(), |acc, (rect, words)| {
                // make the collection of words into single line without any separations (this becomes tricky for English, but for
                // Japanese, it is trivial since there are no such thing as spaces, hence there are tools such as mecab that tries
                // to interpret where to separate for speach and dictionary lookups...)
                format!("{}\n{:?}:{}", acc, rect, words.join(" ")) // previous lines, concatinated with new line in format of rect:words
            });

        write!(
            f,
            "OcrTraitResult {{\n text: {:?}\n lines: {:?}\n rects: {:?}\n }}",
            self.text, str_lines, str_rects
        )
    }
}

impl OcrTraitResult {
    pub fn new() -> Self {
        // NOTE: There will be no default constructor because we want to make sure that we have the necessary data
        //panic!("OcrTraitResult::new() should not be called");
        OcrTraitResult {
            text: "".to_string(),
            lines: vec![],
            rects: HashMap::new(),
        }
    }
}
