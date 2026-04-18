use anyhow::{Context, Result};
use std::path::Path;

const MODEL_DIR: &str = "./models/sarashina2.2/";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let model_dir = Path::new(MODEL_DIR);

    let encoder_path = model_dir.join("encoder_model.onnx");
    let decoder_path = model_dir.join("decoder_model.onnx");

    anyhow::ensure!(encoder_path.exists(), "encoder not found: {}", encoder_path.display());
    anyhow::ensure!(decoder_path.exists(), "decoder not found: {}", decoder_path.display());

    println!("Loading encoder from: {}", encoder_path.display());
    let _encoder = ort::session::Session::builder()?
        .commit_from_file(&encoder_path)
        .context("load encoder session")?;
    println!("Encoder loaded OK");

    println!("Loading decoder from: {}", decoder_path.display());
    let _decoder = ort::session::Session::builder()?
        .commit_from_file(&decoder_path)
        .context("load decoder session")?;
    println!("Decoder loaded OK");

    println!("All sessions created successfully.");
    Ok(())
}
