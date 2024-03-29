extern crate bindgen;
fn main() {
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");

    // Specify the path to the libkakasi header file
    let include_path = if cfg!(target_os = "windows") {
        "./lib/libkakasi.h"
    } else {
        "/usr/include/libkakasi.h"
    };

    // Set library paths based on the target platform and statically link:
    if cfg!(target_os = "windows") {
        // NOTE/IMPORTANT: The path to the library is relative to the project root!!!!!!
        println!(
            "cargo:rustc-link-search=native=prototypes/kakasi-static-libs/lib/x86_64-w64-mingw32"
        );
    } else {
        println!("cargo:rustc-link-search=native=/usr/lib");
    }
    // as the project name states, we are linking against the static library!
    println!("cargo:rustc-link-lib=static=kakasi"); // link against libkakasi.a and/or kakasi.dll/.so (possibly, I'd want "-l:libkakasi.a" instead of "-lkakasi)
    println!("cargo:rustc-link-arg=-lkakasi");    // force link against libkakasi.a (statically linked)
    //println!("cargo:rustc-link-arg=-l:libkakasi.a");    // force link against libkakasi.a (statically linked)

    // Other libs that libkakasi.a depends on:
    println!("cargo:rustc-link-arg=-liconv");    // force link against libkakasi.a (statically linked)

    // Set RUSTFLAGS to include the desired linker search path
    if cfg!(target_os = "windows") {
        // Unlike Linux, MinGW can get a bit confusing on where the lib files are located...
        println!("cargo:rustc-link-arg=-L/mingw64/lib/");
        println!("cargo:rustc-link-arg=-L/c/msys64/mingw64/lib/");
        println!("cargo:rustc-link-arg=-LC:/msys64/mingw64/lib/");
        println!("cargo:rustc-link-arg=-L/usr/lib/");
        println!("cargo:rustc-link-arg=-LC:/msys64/usr/lib/");
    }
    println!("cargo:rustc-link-arg=-lm");
    println!("cargo:rustc-link-arg=-static");

    let bindings = bindgen::Builder::default()
        .header(include_path)
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file("src/bindings.rs")
        .expect("Couldn't write bindings!");
}
