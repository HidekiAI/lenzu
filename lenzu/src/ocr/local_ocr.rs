//! Confidence-gated local OCR using manga-ocr-rs.
//!
//! This module provides the fast, offline OCR path that runs *before* the LLM
//! fallback chain.  When both detection and recognition confidence meet the
//! threshold, results are returned immediately without hitting any remote or
//! local LLM service.

use image::{DynamicImage, GenericImageView};
pub use manga_ocr_rs::{MangaOcr, Recognition};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use super::text_cropper::{CroppedRegion, TextCropper};
use super::text_detection::TextBoundingBox;

/// Confidence threshold — both detection and OCR must be strictly above this
/// to skip the LLM chain.  70% or below = fail (model is guessing).
pub const CONFIDENCE_GATE: f32 = 0.70;

/// Default maximum characters kept from a low-confidence OCR result.
/// Overridden at runtime via `AppConfig::low_conf_max_chars`.
const DEFAULT_LOW_CONF_MAX_CHARS: usize = 64;

/// IoU threshold for clustering overlapping bounding boxes before refinement.
const OVERLAP_CLUSTER_IOU: f32 = 0.15;

/// Refine detected bounding boxes by merging overlapping clusters and
/// re-detecting within each union region.
///
/// 1. Find clusters of boxes that overlap (IoU > `OVERLAP_CLUSTER_IOU`).
/// 2. For each cluster with ≥2 boxes, compute the union bbox, crop that
///    region from the image, and re-run the detector on the crop.
/// 3. If re-detection yields results, use those (remapped to original coords).
///    Otherwise keep the smallest original box from the cluster.
/// 4. Singleton (non-overlapping) boxes pass through unchanged.
///
/// This handles both:
/// - Bubble + text detected as separate overlapping boxes → re-detect finds just the text
/// - Stacked bubbles merged into one tall box → re-detect splits them
pub fn refine_boxes(
    boxes: &[TextBoundingBox],
    image: &DynamicImage,
    detector: &dyn super::text_detection::TextDetector,
) -> Vec<TextBoundingBox> {
    if boxes.len() <= 1 {
        return boxes.to_vec();
    }

    // ── Step 1: cluster overlapping boxes ────────────────────────────────────
    let n = boxes.len();
    let mut cluster_id = vec![0usize; n];
    for i in 0..n {
        cluster_id[i] = i; // each box starts in its own cluster
    }

    // Union-find helpers (path compression only, no rank).
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while parent[r] != r {
            r = parent[r];
        }
        let mut c = i;
        while parent[c] != r {
            let next = parent[c];
            parent[c] = r;
            c = next;
        }
        r
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    }

    for i in 0..n {
        for j in (i + 1)..n {
            let a = &boxes[i];
            let b = &boxes[j];

            let ix1 = a.x1.max(b.x1);
            let iy1 = a.y1.max(b.y1);
            let ix2 = a.x2.min(b.x2);
            let iy2 = a.y2.min(b.y2);

            if ix1 >= ix2 || iy1 >= iy2 {
                continue;
            }

            let inter = (ix2 - ix1) as f32 * (iy2 - iy1) as f32;
            let area_a = (a.x2 - a.x1) as f32 * (a.y2 - a.y1) as f32;
            let area_b = (b.x2 - b.x1) as f32 * (b.y2 - b.y1) as f32;
            let u = area_a + area_b - inter;
            if u > 0.0 && inter / u > OVERLAP_CLUSTER_IOU {
                union(&mut cluster_id, i, j);
            }
        }
    }

    // Group boxes by cluster root.
    let mut clusters: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        clusters.entry(find(&mut cluster_id, i)).or_default().push(i);
    }

    // ── Step 2: refine each cluster ──────────────────────────────────────────

    // Aspect ratio beyond which a singleton looks like merged stacked bubbles.
    // Tategaki (vertical) text is naturally tall, but >3.5:1 is suspicious;
    // yokogaki (horizontal) merge would be very wide, >4:1.
    const SUSPECT_ASPECT: f32 = 3.5;

    // Re-detect within a region and remap coordinates back to original image.
    // Returns `Some(boxes)` if re-detection found anything, `None` otherwise.
    let redetect_region = |x1: u32, y1: u32, w: u32, h: u32| -> Option<Vec<TextBoundingBox>> {
        let sub = image.crop_imm(x1, y1, w, h);
        let sub_gray = sub.grayscale();
        let sub_boxes = detector.detect(&sub_gray);
        if sub_boxes.is_empty() {
            return None;
        }
        Some(sub_boxes.into_iter().map(|b| TextBoundingBox {
            x1: b.x1 + x1,
            y1: b.y1 + y1,
            x2: b.x2 + x1,
            y2: b.y2 + y1,
            confidence: b.confidence,
            contours: b.contours,
        }).collect())
    };

    let mut refined: Vec<TextBoundingBox> = Vec::new();

    for (_root, members) in &clusters {
        if members.len() == 1 {
            let b = &boxes[members[0]];
            let w = b.x2.saturating_sub(b.x1).max(1);
            let h = b.y2.saturating_sub(b.y1).max(1);
            let aspect = w.max(h) as f32 / w.min(h) as f32;

            if aspect > SUSPECT_ASPECT {
                // Suspiciously elongated — might be stacked bubbles merged
                // into one. Re-detect to see if it splits.
                eprintln!(
                    "[local-ocr] refine: singleton ({},{})→({},{}) {}×{} aspect {:.1} > {:.1} — re-detecting",
                    b.x1, b.y1, b.x2, b.y2, w, h, aspect, SUSPECT_ASPECT,
                );
                if let Some(sub_boxes) = redetect_region(b.x1, b.y1, w, h) {
                    if sub_boxes.len() > 1 {
                        eprintln!(
                            "[local-ocr] refine: split into {} boxes",
                            sub_boxes.len(),
                        );
                        refined.extend(sub_boxes);
                    } else {
                        // Re-detect found 1 box — use it (may be tighter).
                        eprintln!("[local-ocr] refine: re-detect returned 1 box (kept)");
                        refined.extend(sub_boxes);
                    }
                } else {
                    eprintln!("[local-ocr] refine: re-detect found nothing, keeping original");
                    refined.push(b.clone());
                }
            } else {
                // Normal singleton — pass through.
                refined.push(b.clone());
            }
            continue;
        }

        // Compute union bbox of the cluster.
        let ux1 = members.iter().map(|&i| boxes[i].x1).min().unwrap();
        let uy1 = members.iter().map(|&i| boxes[i].y1).min().unwrap();
        let ux2 = members.iter().map(|&i| boxes[i].x2).max().unwrap();
        let uy2 = members.iter().map(|&i| boxes[i].y2).max().unwrap();
        let uw = ux2 - ux1;
        let uh = uy2 - uy1;

        eprintln!(
            "[local-ocr] refine: cluster of {} boxes → union ({},{})→({},{}) {}×{}",
            members.len(), ux1, uy1, ux2, uy2, uw, uh,
        );

        if let Some(sub_boxes) = redetect_region(ux1, uy1, uw, uh) {
            eprintln!(
                "[local-ocr] refine: re-detect found {} boxes in union crop (was {})",
                sub_boxes.len(), members.len(),
            );
            refined.extend(sub_boxes);
        } else {
            // Re-detect found nothing — keep the smallest original box
            // (tightest text fit) from the cluster.
            let best = members.iter()
                .min_by_key(|&&i| {
                    let b = &boxes[i];
                    (b.x2 - b.x1) as u64 * (b.y2 - b.y1) as u64
                })
                .unwrap();
            eprintln!(
                "[local-ocr] refine: re-detect found nothing, keeping smallest box ({},{})→({},{})",
                boxes[*best].x1, boxes[*best].y1, boxes[*best].x2, boxes[*best].y2,
            );
            refined.push(boxes[*best].clone());
        }
    }

    refined
}

/// manga-ocr-rs squish-resizes input to 224×224.  Crops whose longest edge
/// exceeds this limit are pre-downscaled with Lanczos3 (better for large
/// reductions) so the final squish is gentler and preserves fine strokes.
/// 2× the model input (448) is the sweet spot: crops ≤448 go through as-is,
/// larger ones get a high-quality pre-shrink.
const OCR_MAX_EDGE: u32 = 448;

/// Result of running the local OCR pipeline on a single crop.
#[derive(Debug, Clone)]
pub struct LocalOcrResult {
    /// Recognised Japanese text (may be truncated if low-confidence).
    pub text: String,
    /// manga-ocr-rs confidence (0.0–1.0).
    pub confidence: f32,
    /// `true` if the decoder ran away without emitting EOS.
    pub truncated: bool,
    /// OCR inference time in milliseconds.
    pub ocr_ms: u128,
    /// The source detection bounding box.
    pub source_box: TextBoundingBox,
}

/// Wraps a shared `MangaOcr` instance for use across capture threads.
pub struct LocalOcrEngine {
    inner: Arc<MangaOcr>,
}

impl LocalOcrEngine {
    /// Load models from the default directory.
    ///
    /// `manga_ocr_rs::default_model_dir()` returns a path baked in at the
    /// crate's build time (the build machine's `~/.cache/manga-ocr-rs/`),
    /// which doesn't exist on a recipient's machine.  Prefer XDG / system
    /// data dirs first; only fall back to the baked path for dev workflows
    /// where the cache lives on the same machine that compiled the binary.
    pub fn new() -> anyhow::Result<Self> {
        let xdg_data = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty())
            .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.local/share")));
        let candidates: Vec<std::path::PathBuf> = [
            xdg_data.map(|d| std::path::PathBuf::from(format!("{d}/lenzu/manga-ocr"))),
            Some(std::path::PathBuf::from("/usr/share/lenzu/manga-ocr")),
            Some(std::path::PathBuf::from("/usr/local/share/lenzu/manga-ocr")),
        ].into_iter().flatten().collect();
        for c in &candidates {
            if c.join("encoder_model.onnx").exists() && c.join("decoder_model.onnx").exists() {
                eprintln!("[OCR] manga-ocr models resolved: {}", c.display());
                return Self::from_path(c);
            }
        }
        let baked = manga_ocr_rs::default_model_dir();
        if baked.join("encoder_model.onnx").exists() {
            eprintln!("[OCR] manga-ocr models from build-time cache: {} (dev workflow)", baked.display());
        }
        Self::from_path(baked)
    }

    /// Load models from a specific directory.
    pub fn from_path(model_dir: &Path) -> anyhow::Result<Self> {
        let ocr = MangaOcr::new(model_dir)?;
        Ok(Self { inner: Arc::new(ocr) })
    }

    /// Wrap an existing shared `MangaOcr` instance.
    pub fn from_arc(ocr: Arc<MangaOcr>) -> Self {
        Self { inner: ocr }
    }

    /// Get a cloneable handle for passing into threads.
    pub fn handle(&self) -> Arc<MangaOcr> {
        Arc::clone(&self.inner)
    }

    /// Run OCR on a single crop image.
    ///
    /// If the crop's longest edge exceeds `OCR_MAX_EDGE`, it is proportionally
    /// downscaled with Lanczos3 before recognition.  This avoids a harsh
    /// squish-resize from large crops directly to 224×224 inside manga-ocr-rs
    /// (which uses bilinear), preserving fine kanji strokes.
    pub fn recognize_crop(&self, crop: &DynamicImage) -> anyhow::Result<(Recognition, u128)> {
        let t = Instant::now();
        let (w, h) = crop.dimensions();
        let longest = w.max(h);
        let rec = if longest > OCR_MAX_EDGE {
            let scale = OCR_MAX_EDGE as f32 / longest as f32;
            let nw = ((w as f32 * scale).round() as u32).max(1);
            let nh = ((h as f32 * scale).round() as u32).max(1);
            eprintln!(
                "[local-ocr] pre-scale crop {}×{} → {}×{} (Lanczos3)",
                w, h, nw, nh,
            );
            let scaled = crop.resize(nw, nh, image::imageops::FilterType::Lanczos3);
            self.inner.recognize_with_score(&scaled)?
        } else {
            self.inner.recognize_with_score(crop)?
        };
        Ok((rec, t.elapsed().as_millis()))
    }

    /// Run the full local-first pipeline on detected text regions.
    ///
    /// Returns `Some(results)` when ALL regions were recognised with sufficient
    /// confidence (both detection >= 71% and OCR >= 71%).  Returns `None` if any
    /// region falls below the gate — the caller should then fall through to the
    /// LLM chain with the full image.
    ///
    /// When `None` is returned, `low_confidence_results` (if provided) is filled
    /// with whatever partial results were obtained — the LLM chain can use these
    /// as hints or discard them.
    pub fn try_local_pipeline(
        &self,
        image: &DynamicImage,
        boxes: &[TextBoundingBox],
        crop_pad: u32,
        min_crop_area: u32,
        low_conf_max_chars: Option<usize>,
    ) -> (Option<Vec<LocalOcrResult>>, Vec<LocalOcrResult>) {
        let max_chars = low_conf_max_chars.unwrap_or(DEFAULT_LOW_CONF_MAX_CHARS);
        let cropper = TextCropper::new(crop_pad, min_crop_area).with_pad_percent(0.10);
        let crops = cropper.crop(image, boxes);

        if crops.is_empty() {
            return (None, vec![]);
        }

        eprintln!("[local-ocr] {} boxes to process", crops.len());

        // Only attempt local OCR on boxes that passed the detection confidence gate.
        // Weak boxes are dropped — they don't drag confident boxes to the LLM chain.
        let (mut confident_crops, weak_crops): (Vec<&CroppedRegion>, Vec<&CroppedRegion>) =
            crops.iter().partition(|c| c.source_box.confidence > CONFIDENCE_GATE);

        if !weak_crops.is_empty() {
            eprintln!(
                "[local-ocr] dropping {} of {} boxes below detection confidence gate ({:.0}%)",
                weak_crops.len(),
                crops.len(),
                CONFIDENCE_GATE * 100.0,
            );
        }

        if confident_crops.is_empty() {
            eprintln!("[local-ocr] no boxes above confidence gate — falling through to LLM");
            return (None, vec![]);
        }

        // Sort right-to-left, top-to-bottom (Japanese manga reading order):
        // primary = descending X midpoint, secondary = ascending Y midpoint.
        confident_crops.sort_by(|a, b| {
            let ax = (a.source_box.x1 + a.source_box.x2) / 2;
            let bx = (b.source_box.x1 + b.source_box.x2) / 2;
            let ay = (a.source_box.y1 + a.source_box.y2) / 2;
            let by = (b.source_box.y1 + b.source_box.y2) / 2;
            bx.cmp(&ax).then(ay.cmp(&by))
        });

        let mut results = Vec::with_capacity(confident_crops.len());
        let mut all_confident = true;

        for (box_idx, crop) in confident_crops.iter().enumerate() {
            let box_num = box_idx + 1;
            match self.recognize_crop(&crop.image) {
                Ok((rec, ms)) => {
                    let ocr_confident = rec.confidence > CONFIDENCE_GATE && !rec.truncated;
                    let char_count = rec.text.chars().count();
                    eprintln!(
                        "[local-ocr] [box {}] ({},{})→({},{}) det:{:.0}% ocr:{:.1}%{} {} chars «{}»  ({} ms)",
                        box_num,
                        crop.source_box.x1, crop.source_box.y1,
                        crop.source_box.x2, crop.source_box.y2,
                        crop.source_box.confidence * 100.0,
                        rec.confidence * 100.0,
                        if rec.truncated { " TRUNCATED" } else { "" },
                        char_count,
                        rec.text,
                        ms,
                    );

                    // Apply truncation for low-confidence long strings.
                    let text = if !ocr_confident && char_count >= max_chars {
                        eprintln!(
                            "[local-ocr] [box {}] truncated {} → {} chars",
                            box_num, char_count, max_chars,
                        );
                        rec.text.chars().take(max_chars).collect()
                    } else {
                        rec.text.clone()
                    };

                    if !ocr_confident {
                        all_confident = false;
                    }

                    results.push(LocalOcrResult {
                        text,
                        confidence: rec.confidence,
                        truncated: rec.truncated,
                        ocr_ms: ms,
                        source_box: crop.source_box.clone(),
                    });
                }
                Err(e) => {
                    eprintln!("[local-ocr] [box {}] recognize failed: {e}", box_num);
                    all_confident = false;
                }
            }
        }

        if all_confident && !results.is_empty() {
            (Some(results.clone()), results)
        } else {
            (None, results)
        }
    }

    /// Like [`try_local_pipeline`] but calls `on_progress` after each box is
    /// recognised, passing the accumulated results so far.  This lets the
    /// caller send incremental previews to the HUD so text grows on screen
    /// rather than appearing all at once (or flashing when boxes are fast).
    pub fn try_local_pipeline_incremental<F>(
        &self,
        image: &DynamicImage,
        boxes: &[TextBoundingBox],
        crop_pad: u32,
        min_crop_area: u32,
        low_conf_max_chars: Option<usize>,
        on_progress: F,
    ) -> (Option<Vec<LocalOcrResult>>, Vec<LocalOcrResult>)
    where
        F: Fn(&[LocalOcrResult]),
    {
        let max_chars = low_conf_max_chars.unwrap_or(DEFAULT_LOW_CONF_MAX_CHARS);
        let cropper = TextCropper::new(crop_pad, min_crop_area).with_pad_percent(0.10);
        let crops = cropper.crop(image, boxes);

        if crops.is_empty() {
            return (None, vec![]);
        }

        eprintln!("[local-ocr] {} boxes to process", crops.len());

        // Weak boxes are dropped — they don't drag confident boxes to the LLM chain.
        let (mut confident_crops, weak_crops): (Vec<&CroppedRegion>, Vec<&CroppedRegion>) =
            crops.iter().partition(|c| c.source_box.confidence > CONFIDENCE_GATE);

        if !weak_crops.is_empty() {
            eprintln!(
                "[local-ocr] dropping {} of {} boxes below detection confidence gate ({:.0}%)",
                weak_crops.len(),
                crops.len(),
                CONFIDENCE_GATE * 100.0,
            );
        }

        if confident_crops.is_empty() {
            eprintln!("[local-ocr] no boxes above confidence gate — falling through to LLM");
            return (None, vec![]);
        }

        // Sort right-to-left, top-to-bottom (Japanese manga reading order):
        // primary = descending X midpoint, secondary = ascending Y midpoint.
        confident_crops.sort_by(|a, b| {
            let ax = (a.source_box.x1 + a.source_box.x2) / 2;
            let bx = (b.source_box.x1 + b.source_box.x2) / 2;
            let ay = (a.source_box.y1 + a.source_box.y2) / 2;
            let by = (b.source_box.y1 + b.source_box.y2) / 2;
            bx.cmp(&ax).then(ay.cmp(&by))
        });

        let mut results = Vec::with_capacity(confident_crops.len());
        let mut all_confident = true;

        for (box_idx, crop) in confident_crops.iter().enumerate() {
            let box_num = box_idx + 1;
            match self.recognize_crop(&crop.image) {
                Ok((rec, ms)) => {
                    let ocr_confident = rec.confidence > CONFIDENCE_GATE && !rec.truncated;
                    let char_count = rec.text.chars().count();
                    eprintln!(
                        "[local-ocr] [box {}] ({},{})→({},{}) det:{:.0}% ocr:{:.1}%{} {} chars «{}»  ({} ms)",
                        box_num,
                        crop.source_box.x1, crop.source_box.y1,
                        crop.source_box.x2, crop.source_box.y2,
                        crop.source_box.confidence * 100.0,
                        rec.confidence * 100.0,
                        if rec.truncated { " TRUNCATED" } else { "" },
                        char_count,
                        rec.text,
                        ms,
                    );

                    let text = if !ocr_confident && char_count >= max_chars {
                        eprintln!(
                            "[local-ocr] [box {}] truncated {} → {} chars",
                            box_num, char_count, max_chars,
                        );
                        rec.text.chars().take(max_chars).collect()
                    } else {
                        rec.text.clone()
                    };

                    if !ocr_confident {
                        all_confident = false;
                    }

                    results.push(LocalOcrResult {
                        text,
                        confidence: rec.confidence,
                        truncated: rec.truncated,
                        ocr_ms: ms,
                        source_box: crop.source_box.clone(),
                    });

                    // Notify caller with accumulated results so far.
                    on_progress(&results);
                }
                Err(e) => {
                    eprintln!("[local-ocr] [box {}] recognize failed: {e}", box_num);
                    all_confident = false;
                }
            }
        }

        if all_confident && !results.is_empty() {
            (Some(results.clone()), results)
        } else {
            (None, results)
        }
    }
}
