fn main() {
    // Put the code that needs to run at compile time here.
    println!("cargo:rerun-if-changed=src/*.rs");
}