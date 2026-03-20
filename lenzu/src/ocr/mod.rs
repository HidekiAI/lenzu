pub mod image_handling;
pub mod ocr_gcloud;
pub mod ocr_tesseract;
pub mod ocr_traits;
#[cfg(target_os = "windows")]
pub mod ocr_winmedia;
pub mod text_detection;
