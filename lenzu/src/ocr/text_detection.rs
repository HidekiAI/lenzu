#[cfg(feature = "onnx")]
use ndarray::Array4;
#[cfg(feature = "onnx")]
use ort::{inputs, Session};
use anyhow::{anyhow, Result};
use image::{DynamicImage, GenericImageView};

pub struct TextDetector {
    #[cfg(feature = "onnx")]
    session: Session,
}

impl TextDetector {
    pub fn new(model_path: &str) -> Result<Self> {
        #[cfg(feature = "onnx")]
        {
            let session = Session::builder()?
                .commit_from_file(model_path)?;
            Ok(TextDetector { session })
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = model_path;
            Err(anyhow!("ONNX feature is not enabled"))
        }
    }

    pub fn detect(&self, image: &DynamicImage) -> Result<Vec<super::ocr_traits::OcrRect>> {
        #[cfg(feature = "onnx")]
        {
            let (width, height) = image.dimensions();
            // YOLOv8 expects 640x640 input typically, or similar depending on the model
            let resized = image.resize_exact(640, 640, image::imageops::FilterType::Lanczos3);
            let rgb = resized.to_rgb8();
            
            // Convert to ndarray [1, 3, 640, 640] and normalize
            let mut input_array = Array4::<f32>::zeros((1, 3, 640, 640));
            for y in 0..640 {
                for x in 0..640 {
                    let pixel = rgb.get_pixel(x, y);
                    input_array[[0, 0, y as usize, x as usize]] = pixel[0] as f32 / 255.0;
                    input_array[[0, 1, y as usize, x as usize]] = pixel[1] as f32 / 255.0;
                    input_array[[0, 2, y as usize, x as usize]] = pixel[2] as f32 / 255.0;
                }
            }

            let outputs = self.session.run(inputs!["images" => input_array]?)?;
            let _output = outputs["output0"].try_extract_tensor::<f32>()?;
            
            // Placeholder for real NMS logic
            let rects = Vec::new();
            Ok(rects)
        }
        #[cfg(not(feature = "onnx"))]
        {
            let _ = image;
            Err(anyhow!("ONNX feature is not enabled"))
        }
    }
}
