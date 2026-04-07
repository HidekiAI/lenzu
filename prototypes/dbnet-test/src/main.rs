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
use image::{DynamicImage, GenericImageView, GrayImage, Luma, Rgb, RgbImage};
use imageproc::contours::find_contours;
use imageproc::distance_transform::Norm;
use imageproc::morphology::dilate;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;

const MEAN: [f32; 3] = [123.675, 116.28, 103.53];
const STD: [f32; 3] = [58.395, 57.12, 57.375];

const INPUT_SIZE: u32 = 640;
const OUT_DIR: &str = "/dev/shm/lenzu";
// CARGO_MANIFEST_DIR = <repo>/prototypes/dbnet-test at compile time → ../../assets = <repo>/assets
const ASSETS_DIR: &str       = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
const SAMPLE_3TEXTS: &str    = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/Unit-test-sample-texts.png");
const SAMPLE_FULLSCREEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/OCR-Demo-JP2EN.png");
const SAMPLE_MODEL: &str     = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/stabrise-text_detection_dbnet_ml_v02_model.onnx");

#[derive(Debug, Clone)]
struct BBox {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

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

fn preprocess(img: &DynamicImage) -> Vec<f32> {
    let resized = img.resize_exact(INPUT_SIZE, INPUT_SIZE, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let n = INPUT_SIZE as usize;
    let mut data = vec![0f32; 3 * n * n];
    for y in 0..n {
        for x in 0..n {
            let px = rgb.get_pixel(x as u32, y as u32);
            for c in 0..3 {
                data[c * n * n + y * n + x] = (px[c] as f32 - MEAN[c]) / STD[c];
            }
        }
    }
    data
}

fn postprocess(prob_map: &[f32], orig_w: u32, orig_h: u32, p: Params) -> Vec<BBox> {
    // Threshold → binary mask
    let mut gray = GrayImage::new(INPUT_SIZE, INPUT_SIZE);
    for y in 0..INPUT_SIZE as usize {
        for x in 0..INPUT_SIZE as usize {
            let v = if prob_map[y * INPUT_SIZE as usize + x] >= p.threshold { 255 } else { 0 };
            gray.put_pixel(x as u32, y as u32, Luma([v]));
        }
    }

    // Dilation: merges nearby blobs and compensates for DBNet's slightly-shrunk training targets
    let mask = if p.dilation > 0 { dilate(&gray, Norm::L1, p.dilation) } else { gray };

    let contours = find_contours::<u32>(&mask);
    let scale_x = orig_w as f32 / INPUT_SIZE as f32;
    let scale_y = orig_h as f32 / INPUT_SIZE as f32;

    let mut boxes = Vec::new();
    for contour in &contours {
        if contour.points.len() < 4 {
            continue;
        }
        let min_x = contour.points.iter().map(|pt| pt.x).min().unwrap();
        let max_x = contour.points.iter().map(|pt| pt.x).max().unwrap();
        let min_y = contour.points.iter().map(|pt| pt.y).min().unwrap();
        let max_y = contour.points.iter().map(|pt| pt.y).max().unwrap();

        // Scale to original image coordinates
        let x = (min_x as f32 * scale_x).round() as u32;
        let y = (min_y as f32 * scale_y).round() as u32;
        let w = ((max_x - min_x) as f32 * scale_x).round() as u32;
        let h = ((max_y - min_y) as f32 * scale_y).round() as u32;

        if w < 5 || h < 5 {
            continue;
        }

        // Expand box by pad_x / pad_y (original-image pixels), clamped to image bounds
        let x = x.saturating_sub(p.pad_x);
        let y = y.saturating_sub(p.pad_y);
        let w = (w + p.pad_x * 2).min(orig_w.saturating_sub(x));
        let h = (h + p.pad_y * 2).min(orig_h.saturating_sub(y));

        boxes.push(BBox { x, y, w, h });
    }

    merge_overlapping(boxes)
}

fn overlaps(a: &BBox, b: &BBox) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

fn union(a: &BBox, b: &BBox) -> BBox {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right  = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    BBox { x, y, w: right - x, h: bottom - y }
}

/// Iteratively merge any two overlapping boxes until no overlaps remain.
fn merge_overlapping(mut boxes: Vec<BBox>) -> Vec<BBox> {
    loop {
        let mut merged = Vec::with_capacity(boxes.len());
        let mut consumed = vec![false; boxes.len()];
        let mut any = false;

        for i in 0..boxes.len() {
            if consumed[i] { continue; }
            let mut current = boxes[i].clone();
            for j in (i + 1)..boxes.len() {
                if consumed[j] { continue; }
                if overlaps(&current, &boxes[j]) {
                    current = union(&current, &boxes[j]);
                    consumed[j] = true;
                    any = true;
                }
            }
            merged.push(current);
        }

        boxes = merged;
        if !any { break; }
    }
    boxes.sort_by(|a, b| a.y.cmp(&b.y).then(a.x.cmp(&b.x)));
    boxes
}

fn draw_boxes(img: &DynamicImage, boxes: &[BBox]) -> RgbImage {
    let mut out = img.to_rgb8();
    let red = Rgb([255u8, 0, 0]);
    for bbox in boxes {
        imageproc::drawing::draw_hollow_rect_mut(
            &mut out,
            imageproc::rect::Rect::at(bbox.x as i32, bbox.y as i32)
                .of_size(bbox.w.max(1), bbox.h.max(1)),
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

fn run_detection(session: &mut Session, det: &DetectionRun, p: Params) -> Result<Vec<BBox>> {
    let (orig_w, orig_h) = det.img.dimensions();
    println!("\n=== {} ({}×{}) ===", det.label, orig_w, orig_h);
    println!("  threshold={:.2}  dilation={}px  pad={}×{}px",
        p.threshold, p.dilation, p.pad_x, p.pad_y);

    let t0 = std::time::Instant::now();
    let flat = preprocess(&det.img);
    println!("  Preprocess : {:?}", t0.elapsed());

    let t1 = std::time::Instant::now();
    let input_name = session.inputs()[0].name().to_string();
    let shape = [1usize, 3, INPUT_SIZE as usize, INPUT_SIZE as usize];
    let input_tensor = Tensor::<f32>::from_array((shape, flat))
        .context("failed to create input tensor")?;
    let outputs = session
        .run(ort::inputs![input_name.as_str() => input_tensor])
        .context("inference failed")?;
    println!("  Inference  : {:?}", t1.elapsed());

    let (out_shape, prob_slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .context("failed to extract output tensor")?;
    println!("  Out shape  : {:?}", out_shape);

    let prob_map: Vec<f32> = prob_slice.to_vec();
    let max_prob = prob_map.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let min_prob = prob_map.iter().cloned().fold(f32::INFINITY, f32::min);
    let above = prob_map.iter().filter(|&&v| v >= p.threshold).count();
    println!("  Prob map   : min={min_prob:.4} max={max_prob:.4} above_thresh={above}/{}", prob_map.len());

    let t2 = std::time::Instant::now();
    let boxes = postprocess(&prob_map, orig_w, orig_h, p);
    println!("  Postprocess: {:?}", t2.elapsed());
    println!("  Boxes found: {}", boxes.len());
    for (i, b) in boxes.iter().enumerate() {
        println!("    [{i:3}] x={} y={} w={} h={}", b.x, b.y, b.w, b.h);
    }

    let annotated = draw_boxes(&det.img, &boxes);
    annotated.save(&det.out_path).context("failed to save output")?;
    println!("  Saved : {}", det.out_path);

    Ok(boxes)
}

fn run_single(image_path: &str, model_path: &str, p: Params) -> Result<()> {
    println!("Image  : {image_path}");
    println!("Model  : {model_path}");

    let img = image::open(image_path).context("failed to open image")?;
    let stem = Path::new(image_path)
        .file_name().unwrap_or_default()
        .to_string_lossy();

    let mut session = Session::builder()
        .context("ort session builder")?
        .commit_from_file(model_path)
        .context("failed to load ONNX model")?;

    run_detection(
        &mut session,
        &DetectionRun {
            label: stem.to_string(),
            img,
            out_path: format!("{OUT_DIR}/{stem}.dbnet_out.png"),
        },
        p,
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

    let mut session = Session::builder()
        .context("ort session builder")?
        .commit_from_file(model_path)
        .context("failed to load ONNX model")?;

    for input in session.inputs().iter() { println!("Model input  : {:?}", input.name()); }
    for output in session.outputs().iter() { println!("Model output : {:?}", output.name()); }

    let b1 = run_detection(
        &mut session,
        &DetectionRun {
            label: "3-texts / native size (lens-crop)".into(),
            out_path: format!("{OUT_DIR}/sample_3texts.png"),
            img: src_3texts,
        },
        p,
    )?;
    println!("  → Expected ~3 boxes, got {}", b1.len());

    let (fw, fh) = src_fullscreen.dimensions();
    let b2 = run_detection(
        &mut session,
        &DetectionRun {
            label: format!("fullscreen / OCR-Demo-JP2EN ({fw}×{fh})"),
            out_path: format!("{OUT_DIR}/sample_fullscreen.png"),
            img: src_fullscreen,
        },
        p,
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
