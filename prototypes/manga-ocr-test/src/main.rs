use anyhow::{Context, Result};
use manga_ocr_test::{default_model_dir, MangaOcr};
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <image> [model_dir]", args[0]);
        std::process::exit(1);
    }

    if let Err(e) = run(&args[1], args.get(2).map(String::as_str)) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(image_path: &str, model_dir_override: Option<&str>) -> Result<()> {
    let model_dir: &Path = match model_dir_override {
        Some(d) => Path::new(d),
        None => default_model_dir(),
    };

    let img = image::open(image_path)
        .with_context(|| format!("open image: {image_path}"))?;
    println!("image : {image_path}  ({}x{})", img.width(), img.height());

    let ocr = MangaOcr::new(model_dir).context("load models")?;

    let t = Instant::now();
    let text = ocr.recognize(&img).context("recognize")?;
    println!("time  : {:?}", t.elapsed());
    println!("text  : {text}");
    Ok(())
}
