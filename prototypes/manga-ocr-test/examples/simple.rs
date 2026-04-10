//! Minimal usage example.
//!
//! ```bash
//! cargo run -p manga-ocr-test --example simple -- assets/manga-ocr-2025 path/to/image.png
//! ```

use manga_ocr_test::MangaOcr;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let model_dir  = args.get(1).map(String::as_str).unwrap_or("assets/manga-ocr-2025");
    let image_path = args.get(2).map(String::as_str).unwrap_or("image.png");

    let ocr = MangaOcr::new(Path::new(model_dir))?;
    let img = image::open(image_path)?;
    let text = ocr.recognize(&img)?;
    println!("{text}");
    Ok(())
}
