//! Integration tests for MangaOcr.
//!
//! Uses three purpose-built fixtures in `assets/`:
//!
//!   Unit-test-horizontal.png  — `データを正確に読み取る`  (600×80, IPAGothic)
//!   Unit-test-tategaki.png    — `言語モデルのテスト`       (70×450, IPAGothic)
//!   Unit-test-tegaki.png      — `手書きの文字サンプル`     (500×80, Dejima-Mincho)
//!
//! Each image is clean black text on white — the same class of input the model
//! was trained on (scanned manga).  Ground truth is known, so EXPECTED_* constants
//! are exact strings, not smoke checks.
//!
//! The real-manga smoke test uses `assets/ubunchu01_02.png` (a real manga page).
//! It only checks that the model produces non-empty Japanese — we don't pin the
//! exact output because it depends on which text region is dominant.
//!
//! ## Model download
//!
//! Run `scripts/setup.sh` (uses curl, no Python needed), or manually:
//! ```bash
//! for f in encoder_model.onnx decoder_model.onnx vocab.txt; do
//!   curl -fL "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/main/$f" \
//!        -o "assets/manga-ocr/$f"
//! done
//! ```

use manga_ocr_test::MangaOcr;
use std::path::Path;
use std::time::Instant;

const MODEL_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/manga-ocr");

const FIXTURE_HORIZONTAL: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/Unit-test-horizontal.png");

const FIXTURE_TATEGAKI: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/Unit-test-tategaki.png");

const FIXTURE_TEGAKI: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/Unit-test-tegaki.png");

const FIXTURE_MANGA: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/ubunchu01_02.png");

// Known-correct expected strings — these are the ACTUAL text in each fixture.
const EXPECTED_HORIZONTAL: &str = "データを正確に読み取る";
const EXPECTED_TATEGAKI:   &str = "言語モデルのテスト";
const EXPECTED_TEGAKI:     &str = "手書きの文字サンプル";

// ── helpers ───────────────────────────────────────────────────────────────────

fn models_present() -> bool {
    Path::new(MODEL_DIR).join("encoder_model.onnx").exists()
}

fn load_ocr() -> MangaOcr {
    MangaOcr::new(Path::new(MODEL_DIR)).expect("load MangaOcr models")
}

fn has_japanese(s: &str) -> bool {
    s.chars().any(|c| ('\u{3000}'..='\u{9FFF}').contains(&c))
}

fn assert_ocr_exact(label: &str, ocr: &MangaOcr, path: &str, expected: &str) {
    let img = image::open(path).unwrap_or_else(|e| panic!("{label}: open {path}: {e}"));
    let t = Instant::now();
    let text = ocr.recognize(&img).unwrap_or_else(|e| panic!("{label}: OCR failed: {e}"));
    let ms = t.elapsed().as_millis();
    println!("{label} ({ms} ms): {text:?}  (expected: {expected:?})");
    assert_eq!(text, expected, "{label}: OCR output does not match ground truth");
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Horizontal printed text — `データを正確に読み取る`.
/// Clean IPAGothic on white, 600×80 px.
#[test]
fn test_horizontal() {
    if !models_present() {
        eprintln!("skip: models not found at {MODEL_DIR}");
        return;
    }
    assert_ocr_exact("horizontal", &load_ocr(), FIXTURE_HORIZONTAL, EXPECTED_HORIZONTAL);
}

/// Tategaki (vertical) text — `言語モデルのテスト`.
/// 70×450 px column; tests centre-pad aspect-ratio preservation.
#[test]
fn test_tategaki() {
    if !models_present() {
        eprintln!("skip: models not found at {MODEL_DIR}");
        return;
    }
    assert_ocr_exact("tategaki", &load_ocr(), FIXTURE_TATEGAKI, EXPECTED_TATEGAKI);
}

/// Tegaki (handwritten-style) text — `手書きの文字サンプル`.
/// Dejima-Mincho font, 500×80 px.
#[test]
fn test_tegaki() {
    if !models_present() {
        eprintln!("skip: models not found at {MODEL_DIR}");
        return;
    }
    assert_ocr_exact("tegaki", &load_ocr(), FIXTURE_TEGAKI, EXPECTED_TEGAKI);
}

/// Real manga page — smoke test only (non-empty Japanese, no crash).
/// Does not pin the exact string because the dominant speech bubble varies.
#[test]
fn test_real_manga_smoke() {
    if !models_present() {
        eprintln!("skip: models not found at {MODEL_DIR}");
        return;
    }
    if !Path::new(FIXTURE_MANGA).exists() {
        eprintln!("skip: {FIXTURE_MANGA} not found");
        return;
    }
    let img = image::open(FIXTURE_MANGA).expect("open manga fixture");
    let ocr = load_ocr();
    let t = Instant::now();
    let text = ocr.recognize(&img).expect("OCR failed");
    let ms = t.elapsed().as_millis();
    println!("manga smoke ({ms} ms): {text:?}");
    assert!(!text.is_empty(), "OCR returned empty string");
    assert!(has_japanese(&text), "no Japanese characters in {text:?}");
}
