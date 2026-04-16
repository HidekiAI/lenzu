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
    pub fn new() -> anyhow::Result<Self> {
        let model_dir = manga_ocr_rs::default_model_dir();
        Self::from_path(model_dir)
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

        eprintln!("[local-ocr] {} boxes detected", crops.len());

        // Only attempt local OCR on boxes that passed the detection confidence gate.
        let (confident_crops, weak_crops): (Vec<&CroppedRegion>, Vec<&CroppedRegion>) =
            crops.iter().partition(|c| c.source_box.confidence > CONFIDENCE_GATE);

        if confident_crops.is_empty() {
            eprintln!(
                "[local-ocr] all {} boxes below detection confidence gate ({:.0}%)",
                crops.len(),
                CONFIDENCE_GATE * 100.0,
            );
            return (None, vec![]);
        }

        if !weak_crops.is_empty() {
            eprintln!(
                "[local-ocr] {} of {} boxes below detection confidence gate — falling through to LLM",
                weak_crops.len(),
                crops.len(),
            );
            return (None, vec![]);
        }

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

        eprintln!("[local-ocr] {} boxes detected", crops.len());

        let (confident_crops, weak_crops): (Vec<&CroppedRegion>, Vec<&CroppedRegion>) =
            crops.iter().partition(|c| c.source_box.confidence > CONFIDENCE_GATE);

        if confident_crops.is_empty() {
            eprintln!(
                "[local-ocr] all {} boxes below detection confidence gate ({:.0}%)",
                crops.len(),
                CONFIDENCE_GATE * 100.0,
            );
            return (None, vec![]);
        }

        if !weak_crops.is_empty() {
            eprintln!(
                "[local-ocr] {} of {} boxes below detection confidence gate — falling through to LLM",
                weak_crops.len(),
                crops.len(),
            );
            return (None, vec![]);
        }

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
