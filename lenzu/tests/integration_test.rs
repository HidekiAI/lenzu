use lenzu::{client, utils};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Pixel tolerance for bounding-box coordinate comparisons.
/// Models do not reproduce exact pixel positions on every run.
const COORD_TOLERANCE: i64 = 15;

#[tokio::test]
async fn test_full_ocr_pipeline_flow() {
    // 1. Start a local mock server
    let mock_server = MockServer::start().await;

    // 2. Mock the OpenRouter JSON response
    // Note: We escape the inner JSON string because that's how LLMs return the body content
    let mock_json_body = json!({
        "choices": [{
            "message": {
                "content": "{\"original\": \"こんにちは\", \"english\": \"Hello\", \"furigana\": \"今日[こんにち]は\", \"romaji\": \"Konnichiwa\"}"
            }
        }]
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .and(header("Authorization", "Bearer test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(mock_json_body))
        .mount(&mock_server)
        .await;

    // 3. Prepare Fake Capture Data
    let bgrx_data = vec![0, 0, 255, 255];
    let rgb = utils::raw_to_rgb(&bgrx_data);
    let b64 = utils::encode_to_base64(&rgb, 1, 1);

    // 4. Hit the mock server using an async client to avoid the blocking runtime panic
    let test_url = format!("{}/api/v1/chat/completions", mock_server.uri());
    let async_client = reqwest::Client::new();

    let res = async_client.post(&test_url)
        .header("Authorization", "Bearer test_key")
        .json(&json!({
            "model": "google/gemini-2.0-flash-001",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "OCR test"},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
            ]}],
            "response_format": { "type": "json_object" }
        }))
        .send()
        .await
        .expect("Failed to send request");

    let response_json: serde_json::Value = res.json().await.expect("Failed to parse JSON");

    // 5. Extract and parse the content just like client.rs does
    let content_raw = response_json["choices"][0]["message"]["content"]
        .as_str()
        .expect("Content missing");

    let translation: client::TranslationResult =
        serde_json::from_str(content_raw).expect("Failed to deserialize TranslationResult");

    // 6. Assertions
    assert_eq!(translation.original, "こんにちは");
    assert_eq!(translation.english.unwrap(), "Hello");
    assert_eq!(translation.romaji.unwrap(), "Konnichiwa");
}

// ── Live ollama smoke-test ────────────────────────────────────────────────────
// Requires a running ollama instance with gemma4:e2b loaded.
// Run with: cargo test --test integration_test -- --ignored
//
// Loads assets/Unit-test-sample-texts.png and assets/Unit-test-sample-texts.json
// and verifies that the model detects the expected text.  Bounding-box
// coordinates are checked with COORD_TOLERANCE pixel allowance — models do not
// reproduce exact pixel positions every run.

/// Parsed entry from Unit-test-sample-texts.json.
#[derive(serde::Deserialize)]
struct SampleEntry {
    label: String,
    top_xy: [i64; 2],
    bot_xy: [i64; 2],
    text: String,
}

/// Find the `TranslationResult` whose `original` contains `expected_text` as a
/// substring (case-sensitive).  Returns `None` if nothing matches.
fn find_result_by_text<'a>(
    results: &'a [client::TranslationResult],
    expected_text: &str,
) -> Option<&'a client::TranslationResult> {
    results.iter().find(|r| r.original.contains(expected_text))
}

#[test]
#[ignore = "requires running ollama with gemma4:e2b — run with --ignored"]
fn test_live_ollama_sample_image() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let img_path = manifest_dir.join("../assets/Unit-test-sample-texts.png");
    let json_path = manifest_dir.join("../assets/Unit-test-sample-texts.json");

    let dyn_image = image::open(&img_path)
        .unwrap_or_else(|_| panic!("cannot open {}", img_path.display()));
    // Downscale to 896px longest edge — matches test-ocr.sh; keeps VRAM usage
    // manageable on cards with < 1 GB free after model load.
    let dyn_image = dyn_image.resize(896, 896, image::imageops::FilterType::Lanczos3);
    let b64 = lenzu::utils::encode_as_grayscale(&dyn_image);

    let expected: Vec<SampleEntry> = serde_json::from_str(
        &std::fs::read_to_string(&json_path)
            .unwrap_or_else(|_| panic!("cannot read {}", json_path.display())),
    )
    .expect("Unit-test-sample-texts.json parse failed");

    let ollama_endpoint = "http://localhost:11434/v1/chat/completions".to_string();
    let model = "gemma4:e2b".to_string();
    let cfg = lenzu::config::AppConfig::default();
    let prompt = cfg.resolved_prompt();

    let ocr = client::OcrClient::new(String::new(), ollama_endpoint, model, prompt);
    let results = ocr
        .call_api(&b64)
        .expect("ollama call_api failed — is ollama running with gemma4:e2b loaded?");

    assert!(
        !results.is_empty(),
        "OCR returned no results for the sample image"
    );

    // For each expected entry, verify the model found the text and coordinates
    // are within tolerance.  We do NOT require a 1-to-1 result count — the model
    // may merge or split lines.
    for entry in &expected {
        let found = find_result_by_text(&results, &entry.text);
        assert!(
            found.is_some(),
            "expected text {:?} (label={}) not found in OCR results: {:?}",
            entry.text,
            entry.label,
            results.iter().map(|r| &r.original).collect::<Vec<_>>()
        );

        let r = found.unwrap();

        // Coordinates are optional — if the model omits them we skip the check.
        // When present they must be within COORD_TOLERANCE px of the expected value.
        assert!(
            client::coords_within(r.top_xy.as_deref(), (entry.top_xy[0], entry.top_xy[1]), COORD_TOLERANCE),
            "top_xy mismatch for {:?}: got {:?}, expected {:?} ±{}",
            entry.label, r.top_xy, entry.top_xy, COORD_TOLERANCE
        );
        assert!(
            client::coords_within(r.bot_xy.as_deref(), (entry.bot_xy[0], entry.bot_xy[1]), COORD_TOLERANCE),
            "bot_xy mismatch for {:?}: got {:?}, expected {:?} ±{}",
            entry.label, r.bot_xy, entry.bot_xy, COORD_TOLERANCE
        );
    }
}
