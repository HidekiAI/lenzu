use anyhow::Error; // the most easiest way to handle errors
use core::result::Result;
use std::fmt::{self, Display, Formatter};

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

#[derive(Debug)]
pub(crate) struct InterpreterTraitResult {
    pub text: String,
    pub lines: Vec<String>,
}
impl InterpreterTraitResult {
    pub(crate) fn new() -> InterpreterTraitResult {
        //panic!("InterpreterTraitResult::new() should not be called");
        InterpreterTraitResult {
            text: "".to_string(),
            lines: vec![],
        }
    }
}

impl Display for InterpreterTraitResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}
