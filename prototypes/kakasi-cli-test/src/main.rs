use anyhow::Error;
use core::result::Result;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn main() {
    let result = call_shell_kakasi("最近人気の\nデスクトップな\nリナックスです!").unwrap();
    println!("\n\nResult:\nText:\n{}\n\nLines:\n{}\n", result.text, result.lines.join("\n"));
}

struct InterpreterTraitResultMock {
    pub text: String,
    pub lines: Vec<String>,
}

pub(crate) fn call_shell_kakasi(text: &str) -> Result<InterpreterTraitResultMock, Error> {
    println!("Running kakasi on text: '{}'", text);
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

    println!("Kakasi process started, setting up stdin...");
    // Write your input data to the stdin stream
    if let Some(stdin) = kakasi_cmd.stdin.as_mut() {
        //let text = "最近人気の\nデスクトップな\nリナックスです!";
        stdin
            .write_all(text.as_bytes())
            .expect("Failed to write to stdin");
    }

    println!("stdin written, waiting for process to complete...");
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

    println!("Process completed, reading stdout and stderr...");
    // Read stdout and stderr
    let stdout_reader = BufReader::new(kakasi_cmd.stdout.expect("Failed to capture stdout"));
    let stderr_reader = BufReader::new(kakasi_cmd.stderr.expect("Failed to capture stderr"));

    let stdout_lines = stdout_reader
        .lines()
        .map(|line| line.unwrap())
        .collect::<Vec<String>>();
    let stderr_lines = stderr_reader
        .lines()
        .map(|line| line.unwrap())
        .collect::<Vec<String>>();
    println!("stdout: {:?}", stdout_lines);
    println!("stderr: {:?}", stderr_lines);

    match stderr_lines.len() {
        0 => {
            // no error
            let text = stdout_lines.join("\n");
            Ok(InterpreterTraitResultMock {
                text,
                lines: stdout_lines,
            })
        }
        _ => Err(anyhow::anyhow!(stderr_lines.join("\n"))),
    }
}
