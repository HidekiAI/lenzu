pub mod capture;
pub mod interpreter;
pub mod ocr;

pub mod cursor_data;
pub mod image_handling;

mod cursor_data;
mod image_handling;

use crate::interpreter::*;
use crate::ocr::*;

use crate::image_handling::*;
use crate::interpreter::interpreter_traits::*;
use crate::ocr::ocr_traits::*; // NOTE: if not declared with 'use', won't be able to use Box<dyn crate::ocr_traits::OcrTrait>

use image::DynamicImage; // the "real" DynamicImage, not the one from imageproc or rusty_tesseract

use cursor_data::CursorData;
// NOTE: We want to use imageproc::image rather than image crate because we want to use imageproc::drawing::draw_text_mut()

use crate::interpreter::interpreter_traits::OcrTrait;