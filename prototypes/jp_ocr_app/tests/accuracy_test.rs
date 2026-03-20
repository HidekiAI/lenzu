use image::open;
use ocr_rs::OcrEngine; // Config removed as we'll pass None
use strsim::levenshtein;

#[test]
fn bench_japanese_accuracy() {
    let engine = OcrEngine::new(
        "models/PP-OCRv5_mobile_det.mnn",
        "models/PP-OCRv5_mobile_rec.mnn",
        "models/japan_dict.txt",
        None,
    )
    .expect("Failed to init test engine");

    let samples = vec![("tests/data/sample1.jpg", "東京都渋谷区")];

    for (path, expected) in samples {
        if let Ok(img) = open(path) {
            if let Ok(results) = engine.recognize(&img) {
                let detected: String = results.iter().map(|b| b.text.as_str()).collect();
                let dist = levenshtein(&detected, expected);
                let acc = 1.0 - (dist as f64 / expected.chars().count() as f64);

                println!(
                    "Target: {} | Got: {} | Accuracy: {:.2}%",
                    expected,
                    detected,
                    acc * 100.0
                );
                assert!(acc > 0.80);
            }
        }
    }
}
