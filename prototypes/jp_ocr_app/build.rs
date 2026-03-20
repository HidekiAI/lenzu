use std::fs;
use std::io::copy;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let out_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let model_path = Path::new(&out_dir).join("models");

    // Force download if the main detection model is missing
    let det_exists = model_path.join("PP-OCRv5_mobile_det.mnn").exists();

    if !det_exists {
        println!("cargo:warning=Downloading PP-OCRv5 Japanese Models...");
        fs::create_dir_all(&model_path)?;

        // Official release assets for the Rust crate
        let models_url =
            "https://github.com/zibo-chen/rust-paddle-ocr/releases/download/v2.1.0/models.zip";
        let dict_url = "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict/japan_dict.txt";

        // 1. Download & Extract Models
        let mut response = reqwest::blocking::get(models_url)?;
        let mut tmpfile = tempfile::tempfile()?;
        copy(&mut response, &mut tmpfile)?;

        let mut archive = zip::ZipArchive::new(tmpfile)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = model_path.join(file.name());
            if file.is_dir() {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    fs::create_dir_all(p)?;
                }
                let mut outfile = fs::File::create(&outpath)?;
                copy(&mut file, &mut outfile)?;
            }
        }

        // 2. Download JP Dictionary
        let mut dict_response = reqwest::blocking::get(dict_url)?;
        let mut dict_file = fs::File::create(model_path.join("japan_dict.txt"))?;
        copy(&mut dict_response, &mut dict_file)?;

        println!(
            "cargo:warning=Models successfully placed in {:?}",
            model_path
        );
    }

    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
