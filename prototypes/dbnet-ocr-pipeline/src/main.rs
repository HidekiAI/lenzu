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

/// Confidence threshold — boxes / OCR results at or below this are considered
/// low-confidence and trigger fallback behaviour.
const CONFIDENCE_GATE: f32 = 0.70;

/// Maximum characters to keep from a low-confidence OCR result.
const LOW_CONF_MAX_CHARS: usize = 32;

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
    let entry = jp_detect::detection_params_for_size(img_w, img_h);
    let threshold = threshold_override.unwrap_or(entry.threshold);
    let dilation = dilation_override.unwrap_or(entry.dilation);
    let pad = pad_override.unwrap_or(entry.pad_x);

    // ── Step 1: Text detection (DBNet) ───────────────────────────────────────
    let t_detect = Instant::now();
    eprintln!("params: threshold={threshold}, dilation={dilation}, pad={pad} (longest edge: {})", img_w.max(img_h));
    let detector = jp_detect::build_text_detector(
        None, threshold, dilation, pad, pad,
    )?.context("DBNet detector not available (onnx feature missing?)")?;

    let mut boxes = detector.detect(&img);
    let detect_ms = t_detect.elapsed().as_millis();
    eprintln!("detection: {} boxes in {} ms", boxes.len(), detect_ms);

    // ── Separate high / low confidence boxes ────────────────────────────────
    // Boxes with confidence <= CONFIDENCE_GATE get retried at different scales.
    let (confident, weak): (Vec<_>, Vec<_>) = boxes.drain(..)
        .partition(|b: &jp_detect::TextBoundingBox| b.confidence > CONFIDENCE_GATE);

    boxes = confident;

    if !weak.is_empty() {
        eprintln!(
            "{} low-confidence boxes (confidence <= {:.0}%) — retrying at different scales",
            weak.len(), CONFIDENCE_GATE * 100.0,
        );

        // Try 0.5x — text becomes larger relative to DBNet's 640×640 input.
        let (sw, sh) = (img_w / 2, img_h / 2);
        if sw > 0 && sh > 0 {
            let se = jp_detect::detection_params_for_size(sw, sh);
            let small_det = jp_detect::build_text_detector(
                None, se.threshold, se.dilation, se.pad_x, se.pad_y,
            )?.context("DBNet detector (0.5x retry)")?;
            let small_img = img.resize_exact(
                sw, sh, image::imageops::FilterType::Triangle,
            );
            let retry_boxes: Vec<_> = small_det.detect(&small_img).into_iter()
                .filter(|b| b.confidence > CONFIDENCE_GATE)
                .map(|b| jp_detect::TextBoundingBox {
                    x1: b.x1 * 2, y1: b.y1 * 2,
                    x2: b.x2 * 2, y2: b.y2 * 2,
                    ..b
                })
                .collect();
            if !retry_boxes.is_empty() {
                eprintln!("  retry 0.5x: recovered {} confident boxes", retry_boxes.len());
                boxes.extend(retry_boxes);
            }
        }

        // Try 1.5x — text becomes smaller relative to DBNet's 640×640 input.
        let (lw, lh) = (img_w * 3 / 2, img_h * 3 / 2);
        let le = jp_detect::detection_params_for_size(lw, lh);
        let large_det = jp_detect::build_text_detector(
            None, le.threshold, le.dilation, le.pad_x, le.pad_y,
        )?.context("DBNet detector (1.5x retry)")?;
        let large_img = img.resize_exact(
            lw, lh, image::imageops::FilterType::Triangle,
        );
        let retry_boxes: Vec<_> = large_det.detect(&large_img).into_iter()
            .filter(|b| b.confidence > CONFIDENCE_GATE)
            .map(|b| jp_detect::TextBoundingBox {
                x1: b.x1 * 2 / 3, y1: b.y1 * 2 / 3,
                x2: b.x2 * 2 / 3, y2: b.y2 * 2 / 3,
                ..b
            })
            .collect();
        if !retry_boxes.is_empty() {
            eprintln!("  retry 1.5x: recovered {} confident boxes", retry_boxes.len());
            boxes.extend(retry_boxes);
        }
    }

    // If still nothing even after retries, try a full-image retry at both scales
    // (covers the case where the original pass returned zero boxes at all).
    if boxes.is_empty() {
        eprintln!("no confident boxes — retrying full detection at different scales");

        let (sw, sh) = (img_w / 2, img_h / 2);
        if sw > 0 && sh > 0 {
            let se = jp_detect::detection_params_for_size(sw, sh);
            let small_det = jp_detect::build_text_detector(
                None, se.threshold, se.dilation, se.pad_x, se.pad_y,
            )?.context("DBNet detector (0.5x full retry)")?;
            let small_img = img.resize_exact(sw, sh, image::imageops::FilterType::Triangle);
            let small_boxes: Vec<_> = small_det.detect(&small_img).into_iter()
                .map(|b| jp_detect::TextBoundingBox {
                    x1: b.x1 * 2, y1: b.y1 * 2,
                    x2: b.x2 * 2, y2: b.y2 * 2,
                    ..b
                })
                .collect();
            if !small_boxes.is_empty() {
                eprintln!("  full retry 0.5x: found {} boxes", small_boxes.len());
                boxes = small_boxes;
            }
        }

        if boxes.is_empty() {
            let (lw, lh) = (img_w * 3 / 2, img_h * 3 / 2);
            let le = jp_detect::detection_params_for_size(lw, lh);
            let large_det = jp_detect::build_text_detector(
                None, le.threshold, le.dilation, le.pad_x, le.pad_y,
            )?.context("DBNet detector (1.5x full retry)")?;
            let large_img = img.resize_exact(lw, lh, image::imageops::FilterType::Triangle);
            let large_boxes: Vec<_> = large_det.detect(&large_img).into_iter()
                .map(|b| jp_detect::TextBoundingBox {
                    x1: b.x1 * 2 / 3, y1: b.y1 * 2 / 3,
                    x2: b.x2 * 2 / 3, y2: b.y2 * 2 / 3,
                    ..b
                })
                .collect();
            if !large_boxes.is_empty() {
                eprintln!("  full retry 1.5x: found {} boxes", large_boxes.len());
                boxes = large_boxes;
            }
        }
    }

    if boxes.is_empty() {
        eprintln!("no text detected (including retries)");
        return Ok(());
    }

    eprintln!("final: {} boxes after confidence filtering + retries", boxes.len());

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

        // 10% proportional padding — prevents edge-character hallucination/hangs.
        let pad_x = (w as f32 * 0.10) as u32;
        let pad_y = (h as f32 * 0.10) as u32;
        let cx1 = bbox.x1.saturating_sub(pad_x);
        let cy1 = bbox.y1.saturating_sub(pad_y);
        let cx2 = (bbox.x2 + pad_x).min(img_w);
        let cy2 = (bbox.y2 + pad_y).min(img_h);
        let crop = img.crop_imm(cx1, cy1, cx2 - cx1, cy2 - cy1);

        // Save individual crop
        if let Some(dir) = save_crops {
            let crop_path = format!("{dir}/crop_{i:02}.png");
            crop.save(&crop_path)
                .with_context(|| format!("save crop: {crop_path}"))?;
        }

        let t = Instant::now();
        let rec = ocr.recognize_with_score(&crop)
            .unwrap_or_else(|e| manga_ocr_rs::Recognition {
                text: format!("ERROR: {e}"),
                score: 0.0,
                raw_confidence: 0.0,
                confidence: 0.0,
                truncated: true,
                token_count: 0,
            });
        let ms = t.elapsed().as_millis();

        // When OCR confidence is low (or decoder truncated without EOS),
        // keep text as-is if short, but truncate long strings as garbage.
        let low_conf_ocr = rec.confidence <= CONFIDENCE_GATE || rec.truncated;
        let char_count = rec.text.chars().count();
        let text = if low_conf_ocr && char_count >= LOW_CONF_MAX_CHARS {
            let truncated: String = rec.text.chars().take(LOW_CONF_MAX_CHARS).collect();
            eprintln!(
                "[{i}] low-confidence OCR ({:.1}%): truncated {char_count} → {} chars",
                rec.confidence * 100.0, LOW_CONF_MAX_CHARS,
            );
            truncated
        } else {
            if low_conf_ocr {
                eprintln!(
                    "[{i}] low-confidence OCR ({:.1}%) but short ({char_count} chars) — keeping as-is",
                    rec.confidence * 100.0,
                );
            }
            rec.text.clone()
        };

        if json_output {
            results.push(serde_json::json!({
                "box": [bbox.x1, bbox.y1, bbox.x2, bbox.y2],
                "size": [w, h],
                "detect_confidence": bbox.confidence,
                "text": text,
                "ocr_confidence": rec.confidence,
                "ocr_truncated": low_conf_ocr && char_count >= LOW_CONF_MAX_CHARS,
                "ms": ms,
            }));
        } else {
            let conf_tag = format!(
                " [det:{:.0}% ocr:{:.0}%{}]",
                bbox.confidence * 100.0,
                rec.confidence * 100.0,
                if low_conf_ocr && char_count >= LOW_CONF_MAX_CHARS { " TRUNCATED" } else { "" },
            );
            println!(
                "[{i}] ({ms} ms) [{},{},{},{}] ({w}x{h}): {text:?}{conf_tag}",
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
