//! manga-ocr-rs prototype — image-to-text OCR for Japanese.
//!
//! Runs [l0wgear/manga-ocr-2025-onnx](https://huggingface.co/l0wgear/manga-ocr-2025-onnx)
//! (kha-white/manga-ocr-base exported to ONNX) via ONNX Runtime.
//! Returns raw Japanese text; no translation, no furigana stripping.
//!
//! Handles both yokogumi (horizontal) and tategaki (vertical) text by
//! centre-padding crops to a square before resizing — preserving character
//! proportions regardless of the crop's aspect ratio.
//!
//! Handles tegaki (handwritten) to the extent of the model's training data.
//!
//! # Crate extraction checklist
//! - [ ] Rename package to `manga-ocr-rs`
//! - [ ] Replace `anyhow` with a `thiserror`-based `MangaOcrError`
//! - [ ] Expose a `TextRecognizer` trait for mock-ability in tests
//! - [ ] Gate on `#[cfg(feature = "onnx")]` (mirror `jp_detect` pattern)
//! - [ ] Publish to crates.io

use anyhow::{Context, Result};
use image::{imageops, DynamicImage, Rgb, RgbImage};
use ort::session::Session;
use ort::value::Tensor as OrtTensor;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::Mutex;

// ── Preprocessing ─────────────────────────────────────────────────────────────
const IMG_SIZE: usize = 224;
const PIXEL_MEAN: f32 = 0.5;
const PIXEL_STD: f32  = 0.5;

// ── Generation (from generation_config.json) ──────────────────────────────────
const DECODER_START_TOKEN_ID: i64 = 2;
const EOS_TOKEN_ID: i64           = 3;
const MAX_DECODE_STEPS: usize     = 300;
const NUM_BEAMS: usize            = 4;
const LENGTH_PENALTY: f32         = 2.0;
const NO_REPEAT_NGRAM: usize      = 3;  // from generation_config.json

// ── Preprocessing ─────────────────────────────────────────────────────────────

/// Preprocess following kha-white/manga-ocr-base's original pipeline:
///
/// 1. Grayscale → RGB  (manga is black-on-white; eliminates colour noise)
/// 2. Centre-pad to square on white canvas  (preserves character proportions
///    for tategaki columns and other non-square crops)
/// 3. Resize to 224×224 with Lanczos
/// 4. Normalise to [-1, 1]  (mean=0.5, std=0.5)
///
/// Returns shape `[1, 3, H, W]` + flat NCHW `Vec<f32>`.
fn preprocess(img: &DynamicImage) -> ([usize; 4], Vec<f32>) {
    let img = img.grayscale().to_rgb8();
    let (w, h) = (img.width(), img.height());
    let side = w.max(h);
    let mut canvas = RgbImage::from_pixel(side, side, Rgb([255u8, 255, 255]));
    imageops::overlay(&mut canvas, &img, ((side - w) / 2) as i64, ((side - h) / 2) as i64);

    let resized = DynamicImage::ImageRgb8(canvas)
        .resize_exact(IMG_SIZE as u32, IMG_SIZE as u32, imageops::FilterType::Lanczos3)
        .to_rgb8();

    let mut flat = vec![0.0f32; 3 * IMG_SIZE * IMG_SIZE];
    for y in 0..IMG_SIZE {
        for x in 0..IMG_SIZE {
            let p = resized.get_pixel(x as u32, y as u32);
            flat[0 * IMG_SIZE * IMG_SIZE + y * IMG_SIZE + x] = (p[0] as f32 / 255.0 - PIXEL_MEAN) / PIXEL_STD;
            flat[1 * IMG_SIZE * IMG_SIZE + y * IMG_SIZE + x] = (p[1] as f32 / 255.0 - PIXEL_MEAN) / PIXEL_STD;
            flat[2 * IMG_SIZE * IMG_SIZE + y * IMG_SIZE + x] = (p[2] as f32 / 255.0 - PIXEL_MEAN) / PIXEL_STD;
        }
    }
    ([1, 3, IMG_SIZE, IMG_SIZE], flat)
}

// ── Vocabulary ────────────────────────────────────────────────────────────────
//
// vocab.txt is a line-indexed file: token ID = line number (0-based).
// Special tokens to skip when decoding:
//   0=[PAD]  1=[UNK]  2=[CLS]/BOS  3=[SEP]/EOS  4=[MASK]  5-14=<unused0-9>

struct VocabDecoder {
    tokens: Vec<String>,   // index = token id
}

impl VocabDecoder {
    fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?;
        let tokens = content.lines().map(str::to_owned).collect();
        Ok(Self { tokens })
    }

    fn decode(&self, ids: &[i64]) -> String {
        ids.iter()
            .filter_map(|&id| {
                let uid = id as usize;
                // Skip all special / control tokens (PAD, UNK, CLS, SEP, MASK, unused0-9)
                if uid < 15 { return None; }
                self.tokens.get(uid)
            })
            // Strip BERT-style continuation prefix "##き" → "き"
            .map(|tok| tok.trim_start_matches("##"))
            .collect()
    }
}

// ── Beam search helpers ───────────────────────────────────────────────────────

fn log_softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let sum_exp: f32 = logits.iter().map(|&x| (x - max).exp()).sum();
    let log_z = sum_exp.ln() + max;
    logits.iter().map(|&x| x - log_z).collect()
}

/// Returns the indices of the top-`k` values in descending order.
fn top_k(log_probs: &[f32], k: usize) -> Vec<(usize, f32)> {
    let mut v: Vec<(usize, f32)> = log_probs.iter().enumerate().map(|(i, &p)| (i, p)).collect();
    v.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    v.truncate(k);
    v
}

/// Block any token whose addition would create a repeated n-gram of size `NO_REPEAT_NGRAM`.
/// Sets those tokens' log-probs to -∞ in-place.
fn apply_no_repeat_ngram(ids: &[i64], log_probs: &mut [f32]) {
    let n = NO_REPEAT_NGRAM;
    if ids.len() < n - 1 || n < 2 { return; }
    let prefix = &ids[ids.len() - (n - 1)..];
    for i in 0..ids.len().saturating_sub(n - 1) {
        if ids[i..i + n - 1] == *prefix {
            let tok = ids[i + n - 1] as usize;
            if tok < log_probs.len() {
                log_probs[tok] = f32::NEG_INFINITY;
            }
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// OCR engine wrapping the encoder + decoder ONNX sessions and vocabulary.
///
/// Construct once (model load is expensive), then call [`recognize`] repeatedly.
/// `Session::run` requires `&mut Session`, so each session is wrapped in a
/// `Mutex` — this lets `MangaOcr` be shared as `Arc<MangaOcr>` across threads
/// (same pattern as `jp_detect::DbNetDetector`).
///
/// [`recognize`]: MangaOcr::recognize
pub struct MangaOcr {
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    vocab:   VocabDecoder,
}

impl MangaOcr {
    /// Load models from `model_dir`.
    ///
    /// Expects these files inside the directory:
    /// - `encoder_model.onnx`
    /// - `decoder_model.onnx`
    /// - `vocab.txt`
    pub fn new(model_dir: &Path) -> Result<Self> {
        let enc_path = model_dir.join("encoder_model.onnx");
        let dec_path = model_dir.join("decoder_model.onnx");
        let tok_path = model_dir.join("vocab.txt");

        let encoder = Session::builder()
            .context("encoder: SessionBuilder")?
            .commit_from_file(&enc_path)
            .with_context(|| format!("encoder: open {}", enc_path.display()))?;

        let decoder = Session::builder()
            .context("decoder: SessionBuilder")?
            .commit_from_file(&dec_path)
            .with_context(|| format!("decoder: open {}", dec_path.display()))?;

        let vocab = VocabDecoder::from_file(&tok_path)?;

        Ok(Self {
            encoder: Mutex::new(encoder),
            decoder: Mutex::new(decoder),
            vocab,
        })
    }

    /// OCR one image crop.  Returns raw Japanese text; no translation.
    ///
    /// Works on any aspect ratio: tategaki (tall), yokogumi (wide), tegaki
    /// (handwritten).  Preprocessing centre-pads to square before resizing.
    ///
    /// Uses beam search (4 beams) matching `generation_config.json`.
    pub fn recognize(&self, img: &DynamicImage) -> Result<String> {
        // ── 1. Encode ────────────────────────────────────────────────────────
        let (pv_shape, pv_data) = preprocess(img);
        let pv_tensor = OrtTensor::<f32>::from_array((pv_shape, pv_data))
            .context("pixel_values tensor")?;

        let (enc_seq_len, hidden_dim, enc_hidden) = {
            let mut enc = self.encoder.lock()
                .map_err(|e| anyhow::anyhow!("encoder lock poisoned: {e}"))?;
            let out = enc.run(ort::inputs!["pixel_values" => pv_tensor])
                .context("encoder run")?;
            let (shape, data) = out["last_hidden_state"]
                .try_extract_tensor::<f32>()
                .context("encoder: extract last_hidden_state")?;
            (shape[1] as usize, shape[2] as usize, data.to_vec())
        };

        // ── 2. Beam search ───────────────────────────────────────────────────
        //
        // Beam state: (token_ids, cumulative_log_prob, done)
        // All beams share the same encoder output (tiled in the batch dimension).

        // Bootstrap: run a single beam with [BOS] to get the top-NUM_BEAMS first tokens.
        let seeds = {
            let mut lp = self.decoder_logprobs_single(
                &[DECODER_START_TOKEN_ID], enc_seq_len, hidden_dim, &enc_hidden,
            )?;
            apply_no_repeat_ngram(&[DECODER_START_TOKEN_ID], &mut lp);
            top_k(&lp, NUM_BEAMS)
        };

        // (ids, score, done)
        let mut beams: Vec<(Vec<i64>, f32, bool)> = seeds.iter()
            .map(|&(tok, lp)| {
                let done = tok as i64 == EOS_TOKEN_ID;
                (vec![DECODER_START_TOKEN_ID, tok as i64], lp, done)
            })
            .collect();
        // Finished beams accumulate here so we can compare against active beams.
        let mut completed: Vec<(Vec<i64>, f32)> = beams.iter()
            .filter(|(_, _, done)| *done)
            .map(|(ids, score, _)| (ids.clone(), *score))
            .collect();

        for _step in 1..MAX_DECODE_STEPS {
            let active: Vec<usize> = beams.iter().enumerate()
                .filter(|(_, (_, _, done))| !*done)
                .map(|(i, _)| i)
                .collect();
            if active.is_empty() { break; }

            let batch = active.len();
            let seq_len = beams[active[0]].0.len(); // all active beams same length

            // Flat input_ids [batch, seq_len]
            let flat_ids: Vec<i64> = active.iter()
                .flat_map(|&i| beams[i].0.iter().copied())
                .collect();

            // Tiled encoder hidden states [batch, enc_seq_len, hidden_dim]
            let flat_enc: Vec<f32> = enc_hidden.iter().copied()
                .cycle()
                .take(batch * enc_seq_len * hidden_dim)
                .collect();

            // Run decoder for all active beams in one call
            let batch_log_probs: Vec<Vec<f32>> = {
                let mut dec = self.decoder.lock()
                    .map_err(|e| anyhow::anyhow!("decoder lock poisoned: {e}"))?;
                let out = dec.run(ort::inputs![
                    "input_ids" =>
                        OrtTensor::<i64>::from_array(([batch, seq_len], flat_ids))
                            .context("input_ids tensor")?,
                    "encoder_hidden_states" =>
                        OrtTensor::<f32>::from_array(([batch, enc_seq_len, hidden_dim], flat_enc))
                            .context("encoder_hidden_states tensor")?
                ]).context("decoder batch run")?;

                let (logits_shape, logits_data) = out["logits"]
                    .try_extract_tensor::<f32>()
                    .context("logits")?;
                let vocab_size = logits_shape[2] as usize;

                (0..batch).map(|b| {
                    let offset = (b * seq_len + seq_len - 1) * vocab_size;
                    log_softmax(&logits_data[offset..offset + vocab_size])
                }).collect()
            };

            // Collect candidates: (active_beam_index, next_token, new_score)
            let mut candidates: Vec<(usize, i64, f32)> = Vec::with_capacity(batch * NUM_BEAMS);
            for (b, &beam_idx) in active.iter().enumerate() {
                let beam_score = beams[beam_idx].1;
                let mut lp = batch_log_probs[b].clone();
                apply_no_repeat_ngram(&beams[beam_idx].0, &mut lp);
                for (tok, lp) in top_k(&lp, NUM_BEAMS) {
                    candidates.push((beam_idx, tok as i64, beam_score + lp));
                }
            }
            candidates.sort_unstable_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(Ordering::Equal));
            candidates.truncate(NUM_BEAMS);

            // Rebuild beams from top candidates
            beams = candidates.iter().map(|&(old_idx, tok, score)| {
                let mut ids = beams[old_idx].0.clone();
                let done = tok == EOS_TOKEN_ID;
                if !done { ids.push(tok); }
                (ids, score, done)
            }).collect();

            for (ids, score, done) in &beams {
                if *done { completed.push((ids.clone(), *score)); }
            }
        }

        // Anything still active at max_steps counts as completed too.
        for (ids, score, done) in &beams {
            if !done { completed.push((ids.clone(), *score)); }
        }

        // ── 3. Pick best beam (length-normalised score) ───────────────────────
        let best_ids = completed.iter()
            .max_by(|(ids_a, score_a), (ids_b, score_b)| {
                let norm = |ids: &[i64], s: f32| s / (ids.len() as f32).powf(LENGTH_PENALTY);
                norm(ids_a, *score_a).partial_cmp(&norm(ids_b, *score_b))
                    .unwrap_or(Ordering::Equal)
            })
            .map(|(ids, _)| ids.as_slice())
            .unwrap_or(&[]);

        // ── 4. Detokenise (skip leading BOS) ─────────────────────────────────
        let decode_ids = if best_ids.first() == Some(&DECODER_START_TOKEN_ID) {
            &best_ids[1..]
        } else {
            best_ids
        };
        Ok(self.vocab.decode(decode_ids))
    }

    /// Run the decoder with a single beam (batch_size=1) and return
    /// log-softmax probabilities for the last token position.
    fn decoder_logprobs_single(
        &self,
        ids: &[i64],
        enc_seq_len: usize,
        hidden_dim: usize,
        enc_hidden: &[f32],
    ) -> Result<Vec<f32>> {
        let seq_len = ids.len();
        let mut dec = self.decoder.lock()
            .map_err(|e| anyhow::anyhow!("decoder lock poisoned: {e}"))?;
        let out = dec.run(ort::inputs![
            "input_ids" =>
                OrtTensor::<i64>::from_array(([1usize, seq_len], ids.to_vec()))
                    .context("input_ids tensor")?,
            "encoder_hidden_states" =>
                OrtTensor::<f32>::from_array(([1usize, enc_seq_len, hidden_dim], enc_hidden.to_vec()))
                    .context("encoder_hidden_states tensor")?
        ]).context("decoder bootstrap run")?;

        let (logits_shape, logits_data) = out["logits"]
            .try_extract_tensor::<f32>()
            .context("logits")?;
        let vocab_size = logits_shape[2] as usize;
        let last = &logits_data[(seq_len - 1) * vocab_size..seq_len * vocab_size];
        Ok(log_softmax(last))
    }
}
