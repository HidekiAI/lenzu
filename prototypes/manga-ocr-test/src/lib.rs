//! Thin re-export of the [`manga_ocr_rs`] crate.
//!
//! This prototype originally contained a full embedded copy of the OCR engine.
//! Now that `manga-ocr-rs` is published on crates.io, this module simply
//! re-exports its public API.

pub use manga_ocr_rs::{default_model_dir, MangaOcr, Recognition};
