extern crate bindgen;
fn main() {
    // Specify the path to the libkakasi header file
    let include_path= if cfg!(target_os = "windows") {
        ".\\mingw-x86_64-libs\\libkakasi.h"
    } else {
        "/usr/include/libkakasi.h"
    };

    // Set library paths based on the target platform and statically link:
    if cfg!(target_os = "windows") {
        // NOTE/IMPORTANT: The path to the library is relative to the project root!!!!!!
        println!("cargo:rustc-link-search=native=prototypes/kakasi-static-libs/mingw-x86_64-libs");
    } else {
        println!("cargo:rustc-link-search=native=/usr/lib");
    }
    println!("cargo:rustc-link-lib=static=kakasi");
    println!("cargo:rustc-link-arg=-liconv");

    let bindings = bindgen::Builder::default()
        .header(include_path)
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file("src/bindings.rs")
        .expect("Couldn't write bindings!");
}
