/// DBNet text detection prototype
///
/// Usage:
///   cargo run -- <image_path> [model] [threshold] [dilation] [pad_x] [pad_y]
///   cargo run -- --test       [model] [threshold] [dilation] [pad_x] [pad_y]
///
/// Defaults:
///   model      = ../../assets/stabrise-text_detection_dbnet_ml_v02_model.onnx
///   threshold  = 0.2
///   dilation   = 16   (mask expansion radius at 640×640 scale — merges nearby blobs)
///   pad_x      = 32   (extra pixels added to each side horizontally, at original-image scale)
///   pad_y      = 32   (extra pixels added to each side vertically,   at original-image scale)
///
/// --test runs two cases:
///   1. Unit-test-sample-texts.png  — 3 distinct text regions (lens-crop simulation)
///   2. OCR-Demo-JP2EN.png          — real game screenshot (fullscreen capture simulation)
///
/// Output PNGs → /dev/shm/lenzu/

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
use jp_detect::{DbNetDetector, TextBoundingBox, TextDetector};
use std::path::Path;

const OUT_DIR: &str = "/dev/shm/lenzu";
// CARGO_MANIFEST_DIR = <repo>/prototypes/dbnet-test at compile time → ../../assets = <repo>/assets
const ASSETS_DIR: &str        = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SAMPLE_3TEXTS: &str     = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/Unit-test-sample-texts.png");
const SAMPLE_FULLSCREEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/OCR-Demo-JP2EN.png");
const SAMPLE_MODEL: &str      = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/stabrise-text_detection_dbnet_ml_v02_model.onnx");

#[derive(Clone, Copy)]
struct Params {
    threshold: f32,
    dilation: u8,
    pad_x: u32,
    pad_y: u32,
}

/// If `path` has no directory component and doesn't exist as-is, look it up in ASSETS_DIR.
fn resolve_path(path: &str) -> String {
    let p = Path::new(path);
    if p.exists() || p.parent().map(|d| d != Path::new("")).unwrap_or(false) {
        path.to_string()
    } else {
        let candidate = format!("{ASSETS_DIR}/{path}");
        if Path::new(&candidate).exists() {
            candidate
        } else {
            path.to_string() // let the caller produce a proper error
        }
    }
}

fn draw_boxes(img: &DynamicImage, boxes: &[TextBoundingBox]) -> RgbImage {
    let mut out = img.to_rgb8();
    let red = Rgb([255u8, 0, 0]);
    for bbox in boxes {
        imageproc::drawing::draw_hollow_rect_mut(
            &mut out,
            imageproc::rect::Rect::at(bbox.x1 as i32, bbox.y1 as i32)
                .of_size(bbox.width().max(1), bbox.height().max(1)),
            red,
        );
    }
    out
}

struct DetectionRun {
    label: String,
    img: DynamicImage,
    out_path: String,
}

fn run_detection(detector: &DbNetDetector, det: &DetectionRun) -> Result<Vec<TextBoundingBox>> {
    let (orig_w, orig_h) = det.img.dimensions();
    println!("\n=== {} ({}×{}) ===", det.label, orig_w, orig_h);

    let t0 = std::time::Instant::now();
    let boxes = detector.detect(&det.img);
    println!("  Detect     : {:?}", t0.elapsed());
    println!("  Boxes found: {}", boxes.len());
    for (i, b) in boxes.iter().enumerate() {
        println!("    [{i:3}] x1={} y1={} x2={} y2={} confidence={:.1}%",
            b.x1, b.y1, b.x2, b.y2, b.confidence * 100.0);
    }

    let annotated = draw_boxes(&det.img, &boxes);
    annotated.save(&det.out_path).context("failed to save output")?;
    println!("  Saved      : {}", det.out_path);

    Ok(boxes)
}

fn run_single(image_path: &str, model_path: &str, p: Params) -> Result<()> {
    println!("Image  : {image_path}");
    println!("Model  : {model_path}");

    let img = image::open(image_path).context("failed to open image")?;
    let stem = Path::new(image_path)
        .file_name().unwrap_or_default()
        .to_string_lossy();

    let detector = DbNetDetector::new(model_path, p.threshold, p.dilation, p.pad_x, p.pad_y)
        .context("failed to create detector")?;

    run_detection(
        &detector,
        &DetectionRun {
            label: stem.to_string(),
            img,
            out_path: format!("{OUT_DIR}/{stem}.dbnet_out.png"),
        },
    )?;
    Ok(())
}

fn run_tests(model_path: &str, p: Params) -> Result<()> {
    println!("Model  : {model_path}");
    println!("3-texts: {SAMPLE_3TEXTS}");
    println!("Full   : {SAMPLE_FULLSCREEN}");
    println!("Out dir: {OUT_DIR}");

    let src_3texts = image::open(SAMPLE_3TEXTS)
        .with_context(|| format!("failed to open {SAMPLE_3TEXTS}"))?;
    let src_fullscreen = image::open(SAMPLE_FULLSCREEN)
        .with_context(|| format!("failed to open {SAMPLE_FULLSCREEN}"))?;

    let detector = DbNetDetector::new(model_path, p.threshold, p.dilation, p.pad_x, p.pad_y)
        .context("failed to create detector")?;

    let b1 = run_detection(
        &detector,
        &DetectionRun {
            label: "3-texts / native size (lens-crop)".into(),
            out_path: format!("{OUT_DIR}/sample_3texts.png"),
            img: src_3texts,
        },
    )?;
    println!("  → Expected ~3 boxes, got {}", b1.len());

    let (fw, fh) = src_fullscreen.dimensions();
    let b2 = run_detection(
        &detector,
        &DetectionRun {
            label: format!("fullscreen / OCR-Demo-JP2EN ({fw}×{fh})"),
            out_path: format!("{OUT_DIR}/sample_fullscreen.png"),
            img: src_fullscreen,
        },
    )?;
    println!("  → Expected 2–4 boxes, got {}", b2.len());

    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let is_test = args.get(1).map(|s| s.as_str()) == Some("--test");
    let (off, image_path) = if is_test {
        (2, None)
    } else if args.len() >= 2 {
        (2, Some(args[1].as_str()))
    } else {
        eprintln!("Usage:");
        eprintln!("  {} <image> [model] [threshold] [dilation] [pad_x] [pad_y]", args[0]);
        eprintln!("  {} --test  [model] [threshold] [dilation] [pad_x] [pad_y]", args[0]);
        std::process::exit(1);
    };

    let model_arg = args.get(off).map(|s| resolve_path(s)).unwrap_or_else(|| SAMPLE_MODEL.to_string());
    let model = model_arg.as_str();
    let p = Params {
        threshold: args.get(off + 1).and_then(|s| s.parse().ok()).unwrap_or(0.2),
        dilation:  args.get(off + 2).and_then(|s| s.parse().ok()).unwrap_or(16),
        pad_x:     args.get(off + 3).and_then(|s| s.parse().ok()).unwrap_or(32),
        pad_y:     args.get(off + 4).and_then(|s| s.parse().ok()).unwrap_or(32),
    };

    if let Err(e) = std::fs::create_dir_all(OUT_DIR) {
        eprintln!("Warning: could not create {OUT_DIR}: {e}");
    }

    let result = if is_test {
        run_tests(model, p)
    } else {
        run_single(image_path.unwrap(), model, p)
    };

    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
