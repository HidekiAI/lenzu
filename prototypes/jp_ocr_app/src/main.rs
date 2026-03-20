use image::open;
use ocr_rs::{OcrEngine, OcrEngineConfig};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    // 1. Setup the Config with the updated method name
    let config = OcrEngineConfig::new().with_threads(4); // Fixed: was with_cpu_threads

    // 2. Initialize Engine
    // Note: Ensure these filenames match the files in your ./models folder
    let engine = OcrEngine::new(
        "models/PP-OCRv5_mobile_det.mnn",
        "models/PP-OCRv5_mobile_rec.mnn",
        "models/japan_dict.txt",
        Some(config),
    )
    .map_err(|e| anyhow::anyhow!("Failed to init engine: {:?}", e))?;

    let img_path = "input_rect.jpg";
    if !Path::new(img_path).exists() {
        println!("Error: Put a Japanese image at ./input_rect.jpg");
        return Ok(());
    }

    // 3. Load and Recognize
    let dynamic_img = open(img_path)?;
    let results = engine
        .recognize(&dynamic_img)
        .map_err(|e| anyhow::anyhow!("OCR Error: {:?}", e))?;

    println!("--- Results ---");
    for block in results {
        println!("[{:.2}] Text: {}", block.bbox.score, block.text);
    }

    Ok(())
}
