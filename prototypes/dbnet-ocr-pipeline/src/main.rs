//! DBNet + manga-ocr-rs pipeline prototype.
//!
//! 1. Loads a manga page (or any image with Japanese text)
//! 2. Runs DBNet (jp_detect) to find text bounding boxes
//! 3. Crops each box from the original image
//! 4. Runs manga-ocr-rs on each crop
//! 5. Prints results with box coordinates and timing
//!
//! Fully offline — no LLM, no cloud API.

use anyhow::{Context, Result};
use image::Rgba;
use imageproc::drawing::{draw_hollow_rect_mut, draw_text_mut};
use imageproc::rect::Rect;
use std::time::Instant;

// ── Detection scale table ────────────────────────────────────────────────────
// DBNet always runs at 640x640 internally, so dilation of N pixels at 640-res
// represents N*(original/640) pixels in the original image — much more blur
// for large inputs.  Scale params inversely with image size.
struct ScaleEntry {
    max_dimension: u32,
    dilation: u8,
    threshold: f32,
    pad: u32,
}

const SCALE_TABLE: &[ScaleEntry] = &[
    ScaleEntry { max_dimension:   800, dilation: 16, threshold: 0.20, pad: 32 },
    ScaleEntry { max_dimension:  1280, dilation: 10, threshold: 0.25, pad: 24 },
    ScaleEntry { max_dimension:  1920, dilation:  6, threshold: 0.35, pad: 16 },
    ScaleEntry { max_dimension:  2560, dilation:  3, threshold: 0.45, pad: 12 },
    ScaleEntry { max_dimension: u32::MAX, dilation: 0, threshold: 0.50, pad:  8 },
];

fn params_for_size(w: u32, h: u32) -> &'static ScaleEntry {
    let longest = w.max(h);
    SCALE_TABLE.iter()
        .find(|e| longest <= e.max_dimension)
        .unwrap_or(SCALE_TABLE.last().unwrap())
}

// Minimum crop size — skip boxes too small to contain readable text.
const MIN_CROP_SIDE: u32 = 16;

// Box outline colours — cycle through these for each detected region.
const BOX_COLORS: [Rgba<u8>; 6] = [
    Rgba([255, 0, 0, 255]),     // red
    Rgba([0, 200, 0, 255]),     // green
    Rgba([0, 100, 255, 255]),   // blue
    Rgba([255, 165, 0, 255]),   // orange
    Rgba([200, 0, 200, 255]),   // magenta
    Rgba([0, 200, 200, 255]),   // cyan
];

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <image> [options]", args[0]);
        eprintln!("  --json                output results as JSON array");
        eprintln!("  --threshold <0.0-1.0> DBNet confidence threshold (auto-scaled by image size)");
        eprintln!("  --dilation <pixels>   DBNet dilation radius (auto-scaled by image size)");
        eprintln!("  --pad <pixels>        crop padding in each direction (auto-scaled by image size)");
        eprintln!("  --save-boxes <path>   save image with bounding box overlay");
        eprintln!("  --save-crops <dir>    save individual crop images to directory");
        std::process::exit(1);
    }

    let image_path = &args[1];
    let json_output = args.iter().any(|a| a == "--json");

    let threshold_override = parse_opt_f32(&args, "--threshold");
    let dilation_override = parse_opt_u8(&args, "--dilation");
    let pad_override = parse_opt_u32(&args, "--pad");
    let save_boxes = parse_opt_str(&args, "--save-boxes");
    let save_crops = parse_opt_str(&args, "--save-crops");

    if let Err(e) = run(
        image_path, json_output,
        threshold_override, dilation_override, pad_override,
        save_boxes.as_deref(), save_crops.as_deref(),
    ) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn parse_opt_f32(args: &[String], flag: &str) -> Option<f32> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_opt_u8(args: &[String], flag: &str) -> Option<u8> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_opt_u32(args: &[String], flag: &str) -> Option<u32> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_opt_str(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn run(
    image_path: &str,
    json_output: bool,
    threshold_override: Option<f32>,
    dilation_override: Option<u8>,
    pad_override: Option<u32>,
    save_boxes: Option<&str>,
    save_crops: Option<&str>,
) -> Result<()> {
    let t_total = Instant::now();

    // ── Load image ───────────────────────────────────────────────────────────
    let img = image::open(image_path)
        .with_context(|| format!("open image: {image_path}"))?;
    let (img_w, img_h) = (img.width(), img.height());
    eprintln!("image: {image_path} ({img_w}x{img_h})");

    // ── Select detection parameters ──────────────────────────────────────────
    // Auto-scale by image size unless explicitly overridden via CLI flags.
    let entry = params_for_size(img_w, img_h);
    let threshold = threshold_override.unwrap_or(entry.threshold);
    let dilation = dilation_override.unwrap_or(entry.dilation);
    let pad = pad_override.unwrap_or(entry.pad);

    // ── Step 1: Text detection (DBNet) ───────────────────────────────────────
    let t_detect = Instant::now();
    eprintln!("params: threshold={threshold}, dilation={dilation}, pad={pad} (longest edge: {})", img_w.max(img_h));
    let detector = jp_detect::build_text_detector(
        None, threshold, dilation, pad, pad,
    )?.context("DBNet detector not available (onnx feature missing?)")?;

    let boxes = detector.detect(&img);
    let detect_ms = t_detect.elapsed().as_millis();
    eprintln!("detection: {} boxes in {} ms", boxes.len(), detect_ms);

    if boxes.is_empty() {
        eprintln!("no text detected");
        return Ok(());
    }

    // ── Save bbox overlay image ─────────────────────────────────────────────
    if let Some(path) = save_boxes {
        save_boxes_image(&img, &boxes, path)?;
        eprintln!("saved bbox overlay: {path}");
    }

    // ── Create crops output directory ───────────────────────────────────────
    if let Some(dir) = save_crops {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create crops dir: {dir}"))?;
    }

    // ── Step 2: OCR each crop ────────────────────────────────────────────────
    let t_ocr_load = Instant::now();
    let ocr = manga_ocr_rs::MangaOcr::new(manga_ocr_rs::default_model_dir())
        .context("load manga-ocr models")?;
    let ocr_load_ms = t_ocr_load.elapsed().as_millis();
    eprintln!("manga-ocr loaded in {} ms", ocr_load_ms);

    let mut results: Vec<serde_json::Value> = Vec::new();

    for (i, bbox) in boxes.iter().enumerate() {
        let w = bbox.width();
        let h = bbox.height();
        if w < MIN_CROP_SIDE || h < MIN_CROP_SIDE {
            eprintln!("[{i}] skip: too small ({w}x{h})");
            continue;
        }

        let crop = img.crop_imm(bbox.x1, bbox.y1, w, h);

        // Save individual crop
        if let Some(dir) = save_crops {
            let crop_path = format!("{dir}/crop_{i:02}.png");
            crop.save(&crop_path)
                .with_context(|| format!("save crop: {crop_path}"))?;
        }

        let t = Instant::now();
        let text = ocr.recognize(&crop)
            .unwrap_or_else(|e| format!("ERROR: {e}"));
        let ms = t.elapsed().as_millis();

        if json_output {
            results.push(serde_json::json!({
                "box": [bbox.x1, bbox.y1, bbox.x2, bbox.y2],
                "size": [w, h],
                "text": text,
                "ms": ms,
            }));
        } else {
            println!(
                "[{i}] ({ms} ms) [{},{},{},{}] ({w}x{h}): {text:?}",
                bbox.x1, bbox.y1, bbox.x2, bbox.y2
            );
        }
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&results)?);
    }

    let total_ms = t_total.elapsed().as_millis();
    eprintln!("\ntotal: {} ms (detect: {} ms, ocr-load: {} ms)", total_ms, detect_ms, ocr_load_ms);

    Ok(())
}

/// Draw coloured bounding box rectangles and index labels on a copy of the image.
fn save_boxes_image(
    img: &image::DynamicImage,
    boxes: &[jp_detect::TextBoundingBox],
    path: &str,
) -> Result<()> {
    let mut canvas = img.to_rgba8();

    // Load a built-in monospace font for labels.
    let font = ab_glyph::FontArc::try_from_slice(include_bytes!("/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"))
        .or_else(|_| ab_glyph::FontArc::try_from_slice(include_bytes!("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf")))
        .context("load label font")?;

    let scale = ab_glyph::PxScale::from(20.0);

    // Draw each box with a 3px-thick border.
    for (i, bbox) in boxes.iter().enumerate() {
        let color = BOX_COLORS[i % BOX_COLORS.len()];

        // Draw 3 nested rectangles for thickness.
        for offset in 0i32..3 {
            let x = (bbox.x1 as i32 - offset).max(0);
            let y = (bbox.y1 as i32 - offset).max(0);
            let w = bbox.width() as i32 + offset * 2;
            let h = bbox.height() as i32 + offset * 2;
            if w > 0 && h > 0 {
                draw_hollow_rect_mut(&mut canvas, Rect::at(x, y).of_size(w as u32, h as u32), color);
            }
        }

        // Draw index label near top-left of box.
        let label = format!("[{}]", i);
        let lx = (bbox.x1 as i32 - 2).max(0);
        let ly = (bbox.y1 as i32 - 22).max(0);
        // Black shadow for readability.
        draw_text_mut(&mut canvas, Rgba([0, 0, 0, 255]), lx + 1, ly + 1, scale, &font, &label);
        draw_text_mut(&mut canvas, color, lx, ly, scale, &font, &label);
    }

    canvas.save(path).with_context(|| format!("save bbox image: {path}"))?;
    Ok(())
}
