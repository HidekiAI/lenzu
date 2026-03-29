use lenzu_client::{client, utils};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
