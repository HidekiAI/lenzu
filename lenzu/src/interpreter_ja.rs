use crate::interpreter_traits::{InterpreterTrait, InterpreterTraitResult}; // so odd that unless I'd  import it in main.rs, this will not be recognized, but once it is recognized, you can comment it in main.rs
use anyhow::{Error, Ok};
use kakasi;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

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
        //self.call_shell_kakasi(text)
    }
}

impl InterpreterJa {
    pub fn new() -> Self {
        InterpreterJa {}
    }

    pub fn call_shell_kakasi(&self, text: &str) -> Result<InterpreterTraitResult, Error> {
        // Create a Command for the 'kakasi' shell command
        //      SET KANWADICTPATH=C:\kakasi\share\kakasi\kanwadict
        //      SET ITAIJIDICTPATH=C:\kakasi\share\kakasi\itaijidict
        //      $ kakasi -JH  -i utf8 -o utf8 <<< "最近人気の\nデスクトップな\nリナックスです!"
        //      さいきんにんきの\nデスクトップな\nリナックスです!
        //      $ kakasi -JH  -i utf8 -o utf8 -f <<< "最近人気の\nデスクトップな\nリナックスです!"
        //      最近[さいきん]人気[にんき]の\nデスクトップな\nリナックスです!
        let mut kakasi_cmd = if cfg!(target_os = "windows") {
            Command::new("..\\..\\kakasi\\bin\\kakasi.exe")
                .arg("-JH")
                .arg("-i")
                .arg("utf8")
                .arg("-o")
                .arg("utf8")
                .arg("-f")
                .stdin(Stdio::piped()) // Set up stdin for input
                .stdout(Stdio::piped()) // Set up stdout for capturing
                .stderr(Stdio::piped()) // Set up stderr for capturing
                .spawn()
                .expect("Failed to start kakasi process")
        } else {
            Command::new("kakasi")
                .arg("-JH")
                .arg("-i")
                .arg("utf8")
                .arg("-o")
                .arg("utf8")
                .arg("-f")
                .stdin(Stdio::piped()) // Set up stdin for input
                .stdout(Stdio::piped()) // Set up stdout for capturing
                .stderr(Stdio::piped()) // Set up stderr for capturing
                .spawn()
                .expect("Failed to start kakasi process")
        };

        // Write your input data to the stdin stream
        if let Some(stdin) = kakasi_cmd.stdin.as_mut() {
            //let text = "最近人気の\nデスクトップな\nリナックスです!";
            stdin
                .write_all(text.as_bytes())
                .expect("Failed to write to stdin");
        }

        // Wait for the process to complete
        let exit_status: std::process::ExitStatus = kakasi_cmd
            .wait()
            .expect("Failed to wait for kakasi process");

        // Read stdout and stderr
        let mut stdout_reader =
            BufReader::new(kakasi_cmd.stdout.expect("Failed to capture stdout"));
        let mut stderr_reader =
            BufReader::new(kakasi_cmd.stderr.expect("Failed to capture stderr"));

        let stdout_lines = stdout_reader
            .lines()
            .map(|line| line.unwrap())
            .collect::<Vec<String>>();
        let stderr_lines = stderr_reader
            .lines()
            .map(|line| line.unwrap())
            .collect::<Vec<String>>();

        match stderr_lines.len() {
            0 => {
                // no error
                let text = stdout_lines.join("\n");
                Ok(InterpreterTraitResult {
                    text,
                    lines: stdout_lines,
                })
            }
            _ => Err(anyhow::anyhow!(stderr_lines.join("\n"))),
        }
    }
}
