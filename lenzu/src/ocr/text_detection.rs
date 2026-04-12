// Re-export the jp_detect crate's public API so the rest of the lenzu codebase
// continues to use `ocr::text_detection::*` without changes.
pub use jp_detect::{
    TextBoundingBox,
    TextDetector,
    build_text_detector,
    closest_box_to_point,
    compute_union_bbox,
    detection_params_for_size,
    DetectionScaleEntry,
    DetectionOutput,
    DEFAULT_SCALE_TABLE,
};

#[cfg(feature = "onnx")]
pub use jp_detect::DbNetDetector;
