use crate::interpreter_traits::{InterpreterTrait, InterpreterTraitResult}; // so odd that unless I'd  import it in main.rs, this will not be recognized, but once it is recognized, you can comment it in main.rs
use anyhow::Error;
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
        let result = self.call_shell_kakasi(text);
        match result {
            Ok(conv_result) => {
                println!("result:\n{}\n{:?}", conv_result.text, conv_result.lines);
                Ok(conv_result)
            }
            Err(e) => Err(e),
        }
    }
}

impl InterpreterJa {
    pub fn new() -> Self {
        InterpreterJa {}
    }

    pub fn call_shell_kakasi(&self, text: &str) -> Result<InterpreterTraitResult, anyhow::Error> {
        // Create a Command for the 'kakasi' shell command
        //      SET KANWADICTPATH=C:\kakasi\share\kakasi\kanwadict
        //      SET ITAIJIDICTPATH=C:\kakasi\share\kakasi\itaijidict
        //      $ kakasi -JH  -i utf8 -o utf8 <<< "最近人気の\nデスクトップな\nリナックスです!"
        //      さいきんにんきの\nデスクトップな\nリナックスです!
        //      $ kakasi -JH  -i utf8 -o utf8 -f <<< "最近人気の\nデスクトップな\nリナックスです!"
        //      最近[さいきん]人気[にんき]の\nデスクトップな\nリナックスです!
        // NOTE: No need to set env vars for DICTS if you pass it as the last parameters to the command
        // Assumes that on both Linux and Windows, kakasi is in the PATH and the DICTS are in the default location
        let mut kakasi_cmd = Command::new("kakasi")
            .arg("-JH")
            .arg("-f")
            .arg("-i")
            .arg("utf8")
            .arg("-o")
            .arg("utf8")
            .stdin(Stdio::piped()) // Set up stdin for input
            .stdout(Stdio::piped()) // Set up stdout for capturing
            .stderr(Stdio::piped()) // Set up stderr for capturing
            .spawn()
            .expect("Failed to start kakasi process");

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
        match exit_status.success() {
            true => {}
            false => {
                return Err(anyhow::anyhow!(
                    "Failed to convert text using kakasi: {}",
                    exit_status
                ));
            }
        }

        // Read stdout and stderr
        let stdout_reader =
            BufReader::new(kakasi_cmd.stdout.expect("Failed to capture stdout"));
        let stderr_reader =
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
