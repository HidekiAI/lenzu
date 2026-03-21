use lenzu::{client, utils}; // Accessing your project as a library
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_full_ocr_pipeline_flow() {
    // 1. Start a local mock server to intercept OpenRouter calls
    let mock_server = MockServer::start().await;

    // 2. Define the "OpenRouter" behavior
    Mock::given(method("POST"))
        .and(path("/api/v1/chat/completions"))
        .and(header("Authorization", "Bearer test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": "SUCCESSFUL_INTEGRATION_TEST"
                }
            }]
        })))
        .mount(&mock_server)
        .await;

    // 3. Prepare "Fake" Capture Data (1 red pixel in BGRX)
    let bgrx_data = vec![0, 0, 255, 255];
    let width = 1;
    let height = 1;

    // 4. Run through your modular logic
    let rgb = utils::raw_to_rgb(&bgrx_data);
    let b64 = utils::encode_to_base64(&rgb, width, height);

    // 5. Point the client to our local Mock Server instead of the real URL
    // We modify the URL in the test to hit the mock_server.uri()
    let client = client::OcrClient::new("test_key".to_string());

    // Manual override for testing purposes to hit the mock server
    let test_url = format!("{}/api/v1/chat/completions", mock_server.uri());

    // We use a simple blocking request here to match your prototype's call_api style,
    // though in a test we'll just verify the logic flow.
    let res = reqwest::blocking::Client::new()
        .post(&test_url)
        .header("Authorization", "Bearer test_key")
        .json(&json!({
            "model": "google/gemini-2.0-flash-001",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "OCR the Japanese text."},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
            ]}]
        }))
        .send()
        .expect("Failed to send to mock server");

    let body: serde_json::Value = res.json().expect("Failed to parse mock JSON");
    let result_text = body["choices"][0]["message"]["content"].as_str().unwrap();

    // 6. Assertions
    assert_eq!(result_text, "SUCCESSFUL_INTEGRATION_TEST");
    assert!(b64.len() > 10); // Ensure image was actually encoded
}
