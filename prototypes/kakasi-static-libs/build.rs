extern crate bindgen;
fn main() {
    println!("cargo:rerun-if-env-changed=RUSTFLAGS");

    let include_path = "/usr/include/libkakasi.h";

    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-search=native=/usr/lib");
    println!("cargo:rustc-link-lib=static=kakasi");
    println!("cargo:rustc-link-arg=-lkakasi");
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
