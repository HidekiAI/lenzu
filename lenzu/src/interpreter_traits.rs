use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use std::collections::HashMap;

pub trait InterpreterTrait {
    // without 'Sized',  won't be able to Box<dyn InterpreterTrait>
    // i.e.     fn new_interpreter(choice: &str) -> Box<dyn crate::interpreter_traits::InterpreterTrait> {...
    fn new() -> Self
    where
        Self: Sized;

    // returns array of Strings of supported languages
    fn init(&self) -> Vec<String>;

    // interpretes/translates the text to locale native  language
    // i.e. Japanese to English, English to Japanese, etc
    fn convert(&self, text: &str) -> Result<InterpreterTraitResult, Error>;
}

pub(crate) struct InterpreterTraitResult {
    pub text: String,
    pub lines: Vec<String>,
}
