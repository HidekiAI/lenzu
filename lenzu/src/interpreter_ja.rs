use crate::interpreter_traits::{InterpreterTrait, InterpreterTraitResult}; // so odd that unless I'd  import it in main.rs, this will not be recognized, but once it is recognized, you can comment it in main.rs
use anyhow::Error;
use kakasi;

pub(crate) struct InterpreterJa {}

impl InterpreterTrait for InterpreterJa {
    fn new() -> Self
    where
        Self: Sized,
    {
        InterpreterJa {}
    }

    fn init(&self) -> Vec<String> {
        vec!["ja".to_string(), "en".to_string()]
    }

    fn convert(&self, text: &str) -> Result<InterpreterTraitResult, Error> {
        let result = kakasi::convert(text);
        let lines = vec![result.hiragana.split('\n').map(|s| s.to_string()).collect()];
        let text = result.hiragana;
        Ok(InterpreterTraitResult { text, lines })
    }
}
