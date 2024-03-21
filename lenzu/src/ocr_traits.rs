use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

// because there seems to be mismatch on the types of images, we need to convert the image to the
pub fn to_imageproc_dynamic_image(image: &[u8]) -> imageproc::image::DynamicImage {
    imageproc::image::load_from_memory(image).unwrap()
}
pub fn to_rusty_tesseract_dynamic_image(image: &[u8]) -> rusty_tesseract::image::DynamicImage {
    rusty_tesseract::image::load_from_memory(image).unwrap()
}

pub trait OcrTrait {
    // without 'Sized',  won't be able to Box<dyn OcrTrait>
    // i.e.     fn new_ocr(choice: &str) -> Box<dyn crate::ocr_traits::OcrTrait> {...
    fn new() -> Self
    where
        Self: Sized;

    // returns array of Strings of supported languages
    fn init(&self) -> Vec<String>;

    fn evaluate_by_paths(&self, image_path: &str) -> Result<OcrTraitResult, Error>;

    fn evaluate(&self, image: &imageproc::image::DynamicImage) -> Result<OcrTraitResult, Error>;
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

// A word is a collection (one or more) of characters and its position
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OcrWord {
    word: String,
    line_index: u16,
    rect: OcrRect,
}
impl OcrWord {
    pub fn new(word: String, line_index: u16, rect: OcrRect) -> Self {
        OcrWord {
            word,
            line_index,
            rect,
        }
    }
    pub fn from(
        word: String,
        line_index: u16,
        x_min: i32,
        y_min: i32,
        width: u32,
        height: u32,
    ) -> Self {
        OcrWord {
            word,
            line_index,
            rect: OcrRect::from(x_min, y_min, width, height),
        }
    }
    pub fn width(&self) -> u32 {
        self.rect.width()
    }
    pub fn height(&self) -> u32 {
        self.rect.height()
    }
    pub fn x_min(&self) -> i32 {
        self.rect.x_min
    }
    pub fn y_min(&self) -> i32 {
        self.rect.y_min
    }
    pub fn rect(&self) -> OcrRect {
        self.rect
    }
    pub fn line_index(&self) -> u16 {
        self.line_index
    }
    pub fn word(&self) -> String {
        self.word.clone()
    }
}

// a line is a collection (one or more) of words
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OcrLine {
    line: Vec<OcrWord>,
}
impl OcrLine {
    pub fn new(words: Vec<OcrWord>) -> Self {
        OcrLine { line: words }
    }
    pub fn add_word(&mut self, word: OcrWord) {
        self.line.push(word);
    }
    pub fn words(&self) -> Vec<OcrWord> {
        self.line.clone()
    }
    pub fn width(&self) -> u32 {
        // get the largest/max width of the words in the line
        self.line
            .iter()
            .fold(0, |acc, word| std::cmp::max(acc, word.width()))
    }
    pub fn height(&self) -> u32 {
        // max height of the words in the line
        self.line
            .iter()
            .fold(0, |acc, word| std::cmp::max(acc, word.height()))
    }
    pub fn x_min(&self) -> i32 {
        // lowest/mimumum x_min of the words in the line
        self.line
            .iter()
            .fold(std::i32::MAX, |acc, word| std::cmp::min(acc, word.x_min()))
    }
    pub fn y_min(&self) -> i32 {
        self.line
            .iter()
            .fold(std::i32::MAX, |acc, word| std::cmp::min(acc, word.y_min()))
    }
}

#[derive(Debug)]
pub(crate) struct OcrTraitResult {
    pub text: String,        // entier text split via newlines (built from lines)
    pub lines: Vec<String>, // each line of text (collection of words), sequentially ordered (up to OCR whether it is horizontal:left-to-right, or vertical:top-to-bottom-left-to-right )
    pub rects: Vec<OcrLine>, // for each (rectangle) block of text (collection of words, see lines)
}

impl Display for OcrTraitResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let lines = self
            .lines
            .iter()
            .fold("".to_string(), |acc, line| acc + line + "\n");
        let rects = self.rects.iter().fold("".to_string(), |acc, rect| {
            acc + &format!("{:?}", rect) + "\n"
        });
        write!(f, "text:{}\nlines:{}\nrects:{}", self.text, lines, rects)
    }
}

impl OcrTraitResult {
    pub fn new() -> Self {
        // NOTE: There will be no default constructor because we want to make sure that we have the necessary data
        //panic!("OcrTraitResult::new() should not be called");
        OcrTraitResult {
            text: "".to_string(),
            lines: vec![],
            rects: vec![],
        }
    }
}
