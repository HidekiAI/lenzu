// build.rs

fn main() {
    // Check if the environment variable "NotMSVC" is set
    if std::env::var("NotMSVC").is_ok() {
        // Handle the NotMSVC condition (e.g., print a message)
        println!("Using MinGW64 (GNU) toolchain.");
    } else {
        // Handle the MSVC condition (if needed)
        println!("Using MSVC toolchain.");
    }
}
