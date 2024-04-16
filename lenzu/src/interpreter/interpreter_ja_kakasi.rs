use super::interpreter_traits::{InterpreterTrait, InterpreterTraitResult}; // so odd that unless I'd  import it in main.rs, this will not be recognized, but once it is recognized, you can comment it in main.rs
use anyhow::Error;

use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

pub(crate) struct InterpreterJaKakasi {}

impl InterpreterTrait for InterpreterJaKakasi {
    fn new() -> Self
    where
        Self: Sized,
    {
        InterpreterJaKakasi {}
    }

    fn init(&self) -> Vec<String> {
        vec!["ja".to_string(), "en".to_string()]
    }

    fn convert(&self, lines: &Vec<String>) -> Result<InterpreterTraitResult, Error> {
        let result = self.call_shell_kakasi(lines);
        match result {
            Ok(conv_result) => {
                println!("result:\n{}\n{:?}", conv_result.text, conv_result.lines);
                Ok(conv_result)
            }
            Err(e) => Err(e),
        }
    }
}

impl InterpreterJaKakasi {
    pub fn new() -> Self {
        InterpreterJaKakasi {}
    }

    pub fn call_shell_kakasi(
        &self,
        lines: &Vec<String>,
    ) -> Result<InterpreterTraitResult, anyhow::Error> {
        // TODO: parallelize this per line, but for now, we'll just join it...
        let text = lines.join("\n");
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

#[cfg(test)]
mod tests {
    // NOTE: This test will correctly render when either "Run Test" or "cargo test", but when "Debug", it will not transcode
    #[test]
    fn test_print_utf8() {
        let japanese_text = "こんにちは、世界！"; // Japanese greeting: "Hello, world!"
        let s = japanese_text.to_string();
        println!("japanese_text: {:?}", s);

        let text_utf8 = "最近人気のデスクトップなリナックスです!";
        println!("text_utf8: {:?}", text_utf8);
    }

    // NOTE: This test will correctly render when either "Run Test" or "cargo test", but when "Debug", it will not transcode
    #[test]
    fn test_print_utf8_escape_codes() {
        // This represents the same Japanese greeting: "こんにちは、世界！"
        let unicode_str = String::from(
            "\u{3053}\u{3093}\u{306B}\u{3061}\u{306F}\u{3001}\u{4E16}\u{754C}\u{FF01}",
        );
        assert!(unicode_str == "こんにちは、世界！");
        println!("unicode_str: {:?}", unicode_str);

        // convert hex \x to unicode point \u{}
        let hex_format: &[u8] = b"\xe6\x9c\x80\xe8\xbf\x91\xe4\xba\xba\xe6\xb0\x97\xe3\x81\xae\xe3\x83\x87\xe3\x82\xb9\xe3\x82\xaf\xe3\x83\x88\xe3\x83\x83\xe3\x83\x97\xe3\x81\xaa\xe3\x83\xaa\xe3\x83\x8a\xe3\x83\x83\xe3\x82\xaf\xe3\x82\xb9\xe3\x81\xa7\xe3\x81\x99!";
        let unescaped_str = String::from_utf8_lossy(hex_format);
        assert!(unescaped_str == "最近人気のデスクトップなリナックスです!");
        println!("{}", unescaped_str);
    }

    #[test]
    fn test_encode_to_SJIS() {
        let text_utf8 = "最近人気のデスクトップなリナックスです!";
        let (enco, _, _) = encoding_rs::SHIFT_JIS.encode(&text_utf8); // from UTF8 -> SJIS
        let text_sjis = enco.into_owned();
        println!("text_sjis: {:?}", text_sjis);
    }

    #[test]
    fn test_decode_from_SJIS() {
        let text_sjis = vec![
            // from SJIS -> UTF8
            0x8D, 0xC5, 0x8B, 0xDF, 0x90, 0x6C, 0x8B, 0x43, 0x82, 0xCC, 0x83, 0x66, 0x83, 0x58,
            0x83, 0x4E, 0x83, 0x67, 0x83, 0x62, 0x83, 0x76, 0x82, 0xC8, 0x83, 0x8A, 0x83, 0x69,
            0x83, 0x62, 0x83, 0x4E, 0x83, 0x58, 0x82, 0xC5, 0x82, 0xB7, 0x21,
        ];
        let (res, _, _) = encoding_rs::SHIFT_JIS.decode(&text_sjis);
        let text_utf8 = res.into_owned();
        println!("text_utf8: {:?}", text_utf8);
    }

    #[test]
    fn test_utf16_to_utf8() {
        let text_utf8 = "最近人気のデスクトップなリナックスです!";
        // encode it as utf16:
        let text_utf16 = encoding_rs::UTF_16BE.encode(text_utf8).0.into_owned();
        println!("text_utf16: {:?}", text_utf16);

        // decode it back to utf8:
        let (res, _, _) = encoding_rs::UTF_16BE.decode(&text_utf16);
        let decoded_utf8 = res.into_owned();

        println!("decoded (utf8): {:?}", decoded_utf8);
        assert!(text_utf8 == decoded_utf8);
    }

    //#[test]
    //fn test_kakasi() {
    //    //$ kakasi -JH  -i utf8 -o utf8 -f <<< "最近人気の\nデスクトップな\nリナックスです!"
    //    println!("test_kakasi; setting  up args...");

    //    // call kakasi_getopt_argv()
    //    //let args = vec!["-JH", "-f", "-o", "utf8", "-i", "utf8", "../../assets/itaijidict", "../../assets/kanwadict"];
    //    let args = vec![
    //        "-JH",
    //        "-f",
    //        "-o",
    //        "utf8",
    //        //"-i", "utf8",     // Uncomment when we determine the cause of hanging when using -i utf8
    //        "../../assets/itaijidict",
    //        "../../assets/kanwadict",
    //    ];
    //    let mut c_args = args
    //        .iter()
    //        .map(|s| std::ffi::CString::new(*s).unwrap())
    //        .collect::<Vec<_>>();
    //    let mut c_args = c_args
    //        .iter_mut()
    //        .map(|s| s.as_ptr() as *mut i8)
    //        .collect::<Vec<_>>();
    //    let argc = c_args.len() as i32;
    //    println!("count: {}, args: {:?}", args.len(), args);
    //    let result = unsafe { kakasi_getopt_argv(argc, c_args.as_mut_ptr()) };
    //    println!("kakasi_getopt_argv() returned: {}", result);

    //    // and finally do...
    //    let text_utf8_slice = "最近人気のデスクトップなリナックスです!";
    //    let text_utf8 = text_utf8_slice.to_string(); // has to be a String, not &str for UTF8?
    //    let (enco, _, _) = encoding_rs::SHIFT_JIS.encode(&text_utf8); // from UTF8 -> SJIS
    //    let text_sjis = enco.into_owned();

    //    // convert it to CStr first before passing it to kakasi_do()
    //    let text_utf8_cstr = match std::ffi::CString::new(text_utf8.clone()) {
    //        Ok(s) => s, // null-terminated string
    //        Err(e) => {
    //            assert!(false, "CString::new() failed: {:?}", e);
    //            std::ffi::CString::new("ERROR: Failed to convert UTF-8 to null-terminated string")
    //                .unwrap()
    //        }
    //    };
    //    println!(
    //        "text_utf8_cstr: b{:?} ({} bytes)\n\tas-str: '{}'\n\tfrom_utf8_lossy(): '{}'",
    //        text_utf8_cstr,
    //        text_utf8_cstr.to_bytes().len(),
    //        text_utf8_cstr.to_str().unwrap(),
    //        String::from_utf8_lossy(text_utf8_cstr.to_bytes())
    //    );

    //    let mut err_inside_unsafe = false;
    //    let result = unsafe {
    //        println!("calling kakasi_do({})...", text_utf8.clone());
    //        let possible_converted_result_buffer_ptr: Option<*mut std::os::raw::c_char> =
    //            if cfg!(usage = "USE_TOKIO_TIMEOUT") {
    //                // async with timeout:
    //                match tokio::time::timeout(std::time::Duration::from_secs(5), async {
    //                    kakasi_do(text_utf8_cstr.as_ptr() as *mut std::os::raw::c_char)
    //                })
    //                .await
    //                {
    //                    Ok(v) => {
    //                        println!("kakasi_do() returned: char* ptr={:?}", v);
    //                        Some(v)
    //                    }
    //                    Err(e) => {
    //                        err_inside_unsafe = true;
    //                        println!("kakasi_do() timed out: {:?}", e);
    //                        None
    //                    }
    //                }
    //            } else {
    //                let buffer_ptr =
    //                    kakasi_do(text_utf8_cstr.to_bytes().as_ptr() as *mut std::os::raw::c_char);
    //                if buffer_ptr.is_null() {
    //                    err_inside_unsafe = true;
    //                    None
    //                } else {
    //                    Some(buffer_ptr)
    //                }
    //            };
    //        println!(
    //            "kakasi_do() returned: char* ptr={:?}",
    //            possible_converted_result_buffer_ptr
    //        );

    //        match possible_converted_result_buffer_ptr {
    //            Some(converted_result_buffer_ptr) => {
    //                let byte_slice = std::slice::from_raw_parts(
    //                    converted_result_buffer_ptr as *const u8,
    //                    text_utf8.len(),
    //                );
    //                let (res, _, _) = encoding_rs::SHIFT_JIS.decode(&byte_slice);
    //                let text_sjis = res.into_owned();
    //                // convert char* pool of buffer to string
    //                let cstr: &std::ffi::CStr =
    //                    std::ffi::CStr::from_ptr(converted_result_buffer_ptr);
    //                println!(
    //                    "cstr: b'{:?}' - {} bytes\n\tfrom_utf8_lossy(): '{}'\n\ttext_sjis: '{}'",
    //                    cstr,
    //                    cstr.to_bytes().len(),
    //                    String::from_utf8_lossy(cstr.to_bytes()),
    //                    text_sjis
    //                );

    //                // convert CStr to &str
    //                let str_result = cstr.to_str();
    //                let ret_str_result = match str_result {
    //                    Ok(s) => {
    //                        println!("str_result: {:?}", s);
    //                        s
    //                    }
    //                    Err(e) => {
    //                        // DO NOT panic here, as we need to free the internal data first
    //                        err_inside_unsafe = true;
    //                        println!("str_result: {:?}", e);
    //                        ""
    //                    }
    //                };
    //                // free internal data allocated by kakasi_do() (to avoid internal leaks)
    //                let free_result = kakasi_free(converted_result_buffer_ptr);
    //                println!("kakasi_free() returned: {}", free_result);

    //                ret_str_result
    //            }
    //            None => {
    //                err_inside_unsafe = true;
    //                println!("kakasi_do() timed out...");
    //                ""
    //            }
    //        }
    //    }; // unsafe
    //    assert!(err_inside_unsafe == false);
    //    println!("{}", result);
    //}
}
