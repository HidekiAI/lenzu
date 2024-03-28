mod bindings;

// This is just a prototype to use libkakasi instead of kakasi
// if you look at the source code of kakasi.c and libkakasi.c, it's exactly
// the same; hence whether you call from CLI (via rust process::Command) or
// from a library (bindgen to libkakasi), the result should be the same.
#[cfg(test)]
mod tests {
    use super::bindings::*;

    #[test]
    fn test_kakasi() {
        //$ kakasi -JH  -i utf8 -o utf8 -f <<< "最近人気の\nデスクトップな\nリナックスです!"
        println!("test_kakasi; setting  up args...");

        // call kakasi_getopt_argv()
        let args = vec!["kakasi", "-JH", "-f", "-i", "utf8", "-o", "utf8"];
        let mut c_args = args
            .iter()
            .map(|s| std::ffi::CString::new(*s).unwrap())
            .collect::<Vec<_>>();
        let mut c_args = c_args
            .iter_mut()
            .map(|s| s.as_ptr() as *mut i8)
            .collect::<Vec<_>>();
        let argc = c_args.len() as i32;
        println!("count: {}, args: {:?}", args.len(), args);
        let result = unsafe { kakasi_getopt_argv(argc, c_args.as_mut_ptr()) };
        println!("kakasi_getopt_argv() returned: {}", result);

        // and finally do...
        let text = "最近人気の\nデスクトップな\nリナックスです!";
        println!("calling kakasi_do({})...", text);
        let result = unsafe {
            let converted_result = kakasi_do(text.as_ptr() as *mut i8);
            println!("kakasi_do() returned: char* ptr={:?}", converted_result);

            // convert char* pool of buffer to string
            let cstr = std::ffi::CStr::from_ptr(converted_result);
            println!("cstr: {:?}", cstr);

            // convert CStr to &str
            let str_result = cstr.to_str().unwrap();
            println!("str_result: {:?}", str_result);

            // free internal data allocated by kakasi_do()
            let free_result = kakasi_free(converted_result);
            println!("kakasi_free() returned: {}", free_result);

            str_result
        };
        println!("{}", result);
    }
}
