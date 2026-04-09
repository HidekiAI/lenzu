use image::{DynamicImage, GenericImageView};

use super::text_detection::TextBoundingBox;

/// A single padded crop of a detected text region, ready for OCR.
pub struct CroppedRegion {
    /// Cropped sub-image, ready for base64 encoding and OCR.
    pub image: DynamicImage,
    /// Top-left corner of this crop in the original (captured) image coordinates.
    /// Used to remap LLM-reported positions and to populate `top_xy`/`bot_xy` from the
    /// DBNet box.
    pub origin_x: u32,
    pub origin_y: u32,
    /// The source bounding box *before* padding was applied (original image coordinates).
    pub source_box: TextBoundingBox,
}

/// Converts detected `TextBoundingBox` list into padded, filtered image crops.
///
/// Keeping this separate from `TextDetector` lets the crop logic be tested and tuned
/// independently of the detection model.
pub struct TextCropper {
    /// Extra pixels added on all four sides of each bounding box before cropping.
    /// Default: 8 px.
    pub pad: u32,
    /// Skip crops whose area (width × height) is below this pixel threshold.
    /// Default: 256 px² (16×16).  Prevents sending furigana / single-character noise.
    pub min_area: u32,
}

impl TextCropper {
    pub fn new(pad: u32, min_area: u32) -> Self {
        TextCropper { pad, min_area }
    }

    /// Crop `image` at each bounding box, apply padding (clamped to image bounds),
    /// filter by `min_area`, and return the surviving crops in the same order as `boxes`.
    pub fn crop(&self, image: &DynamicImage, boxes: &[TextBoundingBox]) -> Vec<CroppedRegion> {
        let (img_w, img_h) = image.dimensions();
        let mut regions = Vec::new();

        for bbox in boxes {
            let x1 = bbox.x1.saturating_sub(self.pad);
            let y1 = bbox.y1.saturating_sub(self.pad);
            let x2 = (bbox.x2 + self.pad).min(img_w);
            let y2 = (bbox.y2 + self.pad).min(img_h);
            let w = x2.saturating_sub(x1);
            let h = y2.saturating_sub(y1);

            if w * h < self.min_area {
                continue;
            }

            let cropped = image.crop_imm(x1, y1, w, h);
            regions.push(CroppedRegion {
                image: cropped,
                origin_x: x1,
                origin_y: y1,
                source_box: bbox.clone(),
            });
        }

        regions
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn solid_image(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(w, h, |_, _| Rgba([255u8, 0, 0, 255])))
    }

    fn bbox(x1: u32, y1: u32, x2: u32, y2: u32) -> TextBoundingBox {
        TextBoundingBox { x1, y1, x2, y2 }
    }

    #[test]
    fn test_crop_basic() {
        let img = solid_image(100, 100);
        let cropper = TextCropper::new(0, 0);
        let boxes = vec![bbox(10, 10, 40, 40)];
        let regions = cropper.crop(&img, &boxes);
        assert_eq!(regions.len(), 1);
        let (w, h) = regions[0].image.dimensions();
        assert_eq!((w, h), (30, 30));
        assert_eq!((regions[0].origin_x, regions[0].origin_y), (10, 10));
    }

    #[test]
    fn test_crop_with_padding() {
        let img = solid_image(100, 100);
        let cropper = TextCropper::new(5, 0);
        let boxes = vec![bbox(10, 10, 40, 40)];
        let regions = cropper.crop(&img, &boxes);
        assert_eq!(regions.len(), 1);
        let (w, h) = regions[0].image.dimensions();
        // pad 5 on each side: x1=5, y1=5, x2=45, y2=45 → 40×40
        assert_eq!((w, h), (40, 40));
        assert_eq!((regions[0].origin_x, regions[0].origin_y), (5, 5));
    }

    #[test]
    fn test_crop_padding_clamped_to_image_bounds() {
        let img = solid_image(50, 50);
        let cropper = TextCropper::new(10, 0);
        let boxes = vec![bbox(0, 0, 20, 20)];
        let regions = cropper.crop(&img, &boxes);
        assert_eq!(regions.len(), 1);
        // x1 = 0.saturating_sub(10) = 0, y1 = 0
        // x2 = min(20+10, 50) = 30, y2 = 30
        let (w, h) = regions[0].image.dimensions();
        assert_eq!((w, h), (30, 30));
    }

    #[test]
    fn test_crop_min_area_filter() {
        let img = solid_image(100, 100);
        let cropper = TextCropper::new(0, 500);
        // 20×20 = 400 px² < 500 → filtered out
        let boxes = vec![bbox(10, 10, 30, 30), bbox(0, 0, 50, 50)];
        let regions = cropper.crop(&img, &boxes);
        assert_eq!(regions.len(), 1);
        let (w, h) = regions[0].image.dimensions();
        assert_eq!((w, h), (50, 50));
    }

    #[test]
    fn test_opt1_union_crop_is_smaller_than_source() {
        use crate::ocr::text_detection::compute_union_bbox;
        // Simulate a 400×400 lens where text only occupies a small region near the top-left.
        // Two boxes covering roughly 60×30 px combined.
        let img = solid_image(400, 400);
        let boxes = vec![bbox(20, 30, 55, 50), bbox(30, 35, 80, 60)];
        let union = compute_union_bbox(&boxes).unwrap();
        let cropper = TextCropper::new(8, 0);
        let crops = cropper.crop(&img, &[union.clone()]);
        assert_eq!(crops.len(), 1);
        let (w, h) = crops[0].image.dimensions();
        // The crop must be well under the full 400×400 lens dimension.
        assert!(w < 400, "crop width {w} should be smaller than the source width 400");
        assert!(h < 400, "crop height {h} should be smaller than the source height 400");
        // The crop must at least cover the union region (before padding).
        assert!(w >= union.x2 - union.x1, "crop must be at least as wide as the union bbox");
        assert!(h >= union.y2 - union.y1, "crop must be at least as tall as the union bbox");
    }

    #[test]
    fn test_source_box_preserved() {
        let img = solid_image(100, 100);
        let cropper = TextCropper::new(2, 0);
        let b = bbox(20, 30, 60, 70);
        let regions = cropper.crop(&img, &[b.clone()]);
        assert_eq!(regions.len(), 1);
        let sb = &regions[0].source_box;
        assert_eq!((sb.x1, sb.y1, sb.x2, sb.y2), (20, 30, 60, 70));
    }
}
