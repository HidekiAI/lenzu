use anyhow::Result;
use image::DynamicImage;
#[cfg(feature = "onnx")]
use image::GenericImageView;

/// Axis-aligned bounding box in the coordinate space of the captured image.
#[derive(Debug, Clone)]
pub struct TextBoundingBox {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl TextBoundingBox {
    pub fn width(&self) -> u32 {
        self.x2.saturating_sub(self.x1)
    }

    pub fn height(&self) -> u32 {
        self.y2.saturating_sub(self.y1)
    }

    /// True when any part of `self` overlaps `other`.
    pub fn intersects(&self, other: &Self) -> bool {
        self.x1 < other.x2 && self.x2 > other.x1 && self.y1 < other.y2 && self.y2 > other.y1
    }
}

/// Swappable text-detection backend.
pub trait TextDetector: Send + Sync {
    fn detect(&self, image: &DynamicImage) -> Vec<TextBoundingBox>;
}

/// Compute the axis-aligned union of all bounding boxes.
/// Returns `None` if `boxes` is empty.
pub fn compute_union_bbox(boxes: &[TextBoundingBox]) -> Option<TextBoundingBox> {
    if boxes.is_empty() {
        return None;
    }
    let x1 = boxes.iter().map(|b| b.x1).min().unwrap();
    let y1 = boxes.iter().map(|b| b.y1).min().unwrap();
    let x2 = boxes.iter().map(|b| b.x2).max().unwrap();
    let y2 = boxes.iter().map(|b| b.y2).max().unwrap();
    Some(TextBoundingBox { x1, y1, x2, y2 })
}

/// Build a text detector from config.
///
/// Returns `Ok(None)` when `model_path` is `None` (detection disabled) or when the
/// binary was built without the `onnx` feature.  Returns `Err` if the model file
/// cannot be loaded.
pub fn build_text_detector(
    model_path: Option<&str>,
    threshold: f32,
    dilation: u8,
    pad_x: u32,
    pad_y: u32,
) -> Result<Option<std::sync::Arc<dyn TextDetector + Send + Sync>>> {
    #[cfg(feature = "onnx")]
    {
        if let Some(path) = model_path {
            let detector = DbNetDetector::new(path, threshold, dilation, pad_x, pad_y)?;
            return Ok(Some(std::sync::Arc::new(detector)
                as std::sync::Arc<dyn TextDetector + Send + Sync>));
        }
    }
    #[cfg(not(feature = "onnx"))]
    if model_path.is_some() {
        eprintln!(
            "[OCR] text_detection_model is configured but this binary was built without \
             the `onnx` feature — text detection disabled"
        );
    }
    let _ = (threshold, dilation, pad_x, pad_y); // suppress unused-var warnings without onnx
    Ok(None)
}

// ── DBNet implementation ──────────────────────────────────────────────────────

#[cfg(feature = "onnx")]
const INPUT_SIZE: u32 = 640;
#[cfg(feature = "onnx")]
const MEAN: [f32; 3] = [123.675, 116.28, 103.53];
#[cfg(feature = "onnx")]
const STD: [f32; 3] = [58.395, 57.12, 57.375];

/// DBNet text detector backed by an ONNX session.
///
/// The session is wrapped in `Mutex` because `ort` 2.0-rc.10 requires `&mut Session`
/// for `run()`.  The `Mutex` gives interior mutability while satisfying `Send + Sync`.
#[cfg(feature = "onnx")]
pub struct DbNetDetector {
    session: std::sync::Mutex<ort::session::Session>,
    input_name: String,
    threshold: f32,
    dilation: u8,
    pad_x: u32,
    pad_y: u32,
}

#[cfg(feature = "onnx")]
impl DbNetDetector {
    pub fn new(
        model_path: &str,
        threshold: f32,
        dilation: u8,
        pad_x: u32,
        pad_y: u32,
    ) -> Result<Self> {
        let session = ort::session::Session::builder()
            .map_err(|e| anyhow::anyhow!("ort session builder failed: {e}"))?
            .commit_from_file(model_path)
            .map_err(|e| anyhow::anyhow!("failed to load ONNX model from '{model_path}': {e}"))?;
        let input_name = session.inputs()[0].name().to_string();
        Ok(Self {
            session: std::sync::Mutex::new(session),
            input_name,
            threshold,
            dilation,
            pad_x,
            pad_y,
        })
    }
}

#[cfg(feature = "onnx")]
impl TextDetector for DbNetDetector {
    fn detect(&self, image: &DynamicImage) -> Vec<TextBoundingBox> {
        let (orig_w, orig_h) = image.dimensions();
        let flat = preprocess(image);

        let shape = [1usize, 3, INPUT_SIZE as usize, INPUT_SIZE as usize];
        let input_tensor =
            match ort::value::Tensor::<f32>::from_array((shape, flat)) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[DBNet] failed to create input tensor: {e}");
                    return vec![];
                }
            };

        let mut session = self.session.lock().unwrap();
        let outputs =
            match session.run(ort::inputs![self.input_name.as_str() => input_tensor]) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("[DBNet] inference failed: {e}");
                    return vec![];
                }
            };

        let prob_vec = match outputs[0].try_extract_tensor::<f32>() {
            Ok((_, slice)) => slice.to_vec(),
            Err(e) => {
                eprintln!("[DBNet] failed to extract output tensor: {e}");
                return vec![];
            }
        };

        postprocess(&prob_vec, orig_w, orig_h, self.threshold, self.dilation, self.pad_x, self.pad_y)
    }
}

#[cfg(feature = "onnx")]
fn preprocess(img: &DynamicImage) -> Vec<f32> {
    let resized =
        img.resize_exact(INPUT_SIZE, INPUT_SIZE, image::imageops::FilterType::Triangle);
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

#[cfg(feature = "onnx")]
fn postprocess(
    prob_map: &[f32],
    orig_w: u32,
    orig_h: u32,
    threshold: f32,
    dilation: u8,
    pad_x: u32,
    pad_y: u32,
) -> Vec<TextBoundingBox> {
    use image::{GrayImage, Luma};
    use imageproc::contours::find_contours;
    use imageproc::distance_transform::Norm;
    use imageproc::morphology::dilate;

    // Threshold → binary mask
    let mut gray = GrayImage::new(INPUT_SIZE, INPUT_SIZE);
    for y in 0..INPUT_SIZE as usize {
        for x in 0..INPUT_SIZE as usize {
            let v = if prob_map[y * INPUT_SIZE as usize + x] >= threshold {
                255
            } else {
                0
            };
            gray.put_pixel(x as u32, y as u32, Luma([v]));
        }
    }

    // Dilation: merge nearby blobs, compensate for DBNet's shrunk training targets
    let mask = if dilation > 0 {
        dilate(&gray, Norm::L1, dilation)
    } else {
        gray
    };

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

        if max_x.saturating_sub(min_x) < 5 || max_y.saturating_sub(min_y) < 5 {
            continue;
        }

        // Scale back to original image coordinates
        let x1 = (min_x as f32 * scale_x).round() as u32;
        let y1 = (min_y as f32 * scale_y).round() as u32;
        let x2 = (max_x as f32 * scale_x).round() as u32;
        let y2 = (max_y as f32 * scale_y).round() as u32;

        // Expand by pad_x / pad_y, clamped to image bounds
        let x1 = x1.saturating_sub(pad_x);
        let y1 = y1.saturating_sub(pad_y);
        let x2 = (x2 + pad_x).min(orig_w);
        let y2 = (y2 + pad_y).min(orig_h);

        boxes.push(TextBoundingBox { x1, y1, x2, y2 });
    }

    merge_overlapping(boxes)
}

/// Boxes are considered overlapping if they touch or are within `MERGE_GAP` pixels of each other.
/// A small gap absorbs 1–2 px rounding noise from the probability map without
/// accidentally merging genuinely separate regions (which are always ≫ 4 px apart).
#[cfg(feature = "onnx")]
const MERGE_GAP: u32 = 4;

#[cfg(feature = "onnx")]
fn overlaps(a: &TextBoundingBox, b: &TextBoundingBox) -> bool {
    a.x1 < b.x2 + MERGE_GAP
        && a.x2 + MERGE_GAP > b.x1
        && a.y1 < b.y2 + MERGE_GAP
        && a.y2 + MERGE_GAP > b.y1
}

#[cfg(feature = "onnx")]
fn union_of(a: &TextBoundingBox, b: &TextBoundingBox) -> TextBoundingBox {
    TextBoundingBox {
        x1: a.x1.min(b.x1),
        y1: a.y1.min(b.y1),
        x2: a.x2.max(b.x2),
        y2: a.y2.max(b.y2),
    }
}

/// Iteratively merge overlapping boxes until no overlaps remain, then sort top-to-bottom.
#[cfg(feature = "onnx")]
fn merge_overlapping(mut boxes: Vec<TextBoundingBox>) -> Vec<TextBoundingBox> {
    loop {
        let mut merged = Vec::with_capacity(boxes.len());
        let mut consumed = vec![false; boxes.len()];
        let mut any = false;

        for i in 0..boxes.len() {
            if consumed[i] {
                continue;
            }
            let mut current = boxes[i].clone();
            for j in (i + 1)..boxes.len() {
                if consumed[j] {
                    continue;
                }
                if overlaps(&current, &boxes[j]) {
                    current = union_of(&current, &boxes[j]);
                    consumed[j] = true;
                    any = true;
                }
            }
            merged.push(current);
        }

        boxes = merged;
        if !any {
            break;
        }
    }
    boxes.sort_by(|a, b| a.y1.cmp(&b.y1).then(a.x1.cmp(&b.x1)));
    boxes
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn bbox(x1: u32, y1: u32, x2: u32, y2: u32) -> TextBoundingBox {
        TextBoundingBox { x1, y1, x2, y2 }
    }

    #[test]
    fn test_compute_union_empty() {
        assert!(compute_union_bbox(&[]).is_none());
    }

    #[test]
    fn test_compute_union_single() {
        let b = compute_union_bbox(&[bbox(10, 20, 30, 40)]).unwrap();
        assert_eq!((b.x1, b.y1, b.x2, b.y2), (10, 20, 30, 40));
    }

    #[test]
    fn test_compute_union_multiple() {
        let boxes = vec![bbox(0, 0, 10, 10), bbox(5, 5, 20, 20), bbox(15, 0, 25, 5)];
        let u = compute_union_bbox(&boxes).unwrap();
        assert_eq!((u.x1, u.y1, u.x2, u.y2), (0, 0, 25, 20));
    }

    #[test]
    fn test_intersects() {
        let a = bbox(0, 0, 10, 10);
        let b = bbox(5, 5, 15, 15);
        let c = bbox(11, 11, 20, 20);
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn test_bbox_dimensions() {
        let b = bbox(5, 10, 25, 40);
        assert_eq!(b.width(), 20);
        assert_eq!(b.height(), 30);
    }

    #[cfg(feature = "onnx")]
    #[test]
    fn test_merge_overlapping_collapses() {
        let boxes = vec![
            bbox(0, 0, 10, 10),
            bbox(8, 8, 20, 20), // overlaps first
            bbox(50, 50, 60, 60),
        ];
        let merged = merge_overlapping(boxes);
        assert_eq!(merged.len(), 2, "two non-overlapping groups should remain");
        // Union of first two
        let u = &merged[0];
        assert_eq!((u.x1, u.y1, u.x2, u.y2), (0, 0, 20, 20));
    }

    /// Lens-crop simulation: three separate text columns/rows in Unit-test-sample-texts.png.
    /// Expected output validated by the dbnet-test prototype (threshold=0.2, dilation=16, pad=32×32).
    #[cfg(feature = "onnx")]
    #[test]
    fn test_detect_lens_crop_returns_three_boxes() {
        let root = env!("CARGO_MANIFEST_DIR");
        let model = format!("{root}/../assets/stabrise-text_detection_dbnet_ml_v02_model.onnx");
        let image_path = format!("{root}/../assets/Unit-test-sample-texts.png");
        let detector = DbNetDetector::new(&model, 0.2, 16, 32, 32)
            .expect("failed to load DBNet model");
        let image = image::open(&image_path).expect("failed to open Unit-test-sample-texts.png");
        let boxes = detector.detect(&image);
        assert_eq!(boxes.len(), 3, "expected 3 text regions, got {}: {boxes:?}", boxes.len());
    }

    /// Fullscreen capture simulation: two dialogue regions in OCR-Demo-JP2EN.png.
    /// Expected output validated by the dbnet-test prototype (ort =2.0.0-rc.10,
    /// threshold=0.2, dilation=16, pad=32×32): header + dialogue merge into one box,
    /// subtitle is the second. Cargo.toml pins ort to rc.10 exactly to keep this stable.
    #[cfg(feature = "onnx")]
    #[test]
    fn test_detect_fullscreen_returns_two_boxes() {
        let root = env!("CARGO_MANIFEST_DIR");
        let model = format!("{root}/../assets/stabrise-text_detection_dbnet_ml_v02_model.onnx");
        let image_path = format!("{root}/../assets/OCR-Demo-JP2EN.png");
        let detector = DbNetDetector::new(&model, 0.2, 16, 32, 32)
            .expect("failed to load DBNet model");
        let image = image::open(&image_path).expect("failed to open OCR-Demo-JP2EN.png");
        let boxes = detector.detect(&image);
        assert_eq!(boxes.len(), 2, "expected 2 text regions, got {}: {boxes:?}", boxes.len());
    }
}
