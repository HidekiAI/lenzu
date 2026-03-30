use chrono::Local;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;

const HISTORY_PATH: &str = "/dev/shm/ocr_history.txt";

pub struct OcrClient {
    api_key: String,
    endpoint: String,
    model: String,
    prompt: String,
    client: Client,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone, PartialEq)]
pub struct TranslationResult {
    pub original: String,
    pub furigana: Option<String>,
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub top_xy: Option<String>,  // upper-left bounding box corner
    pub bot_xy: Option<String>,  // lower-right bounding box corner
    pub debug_info: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OpenRouterResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
}

#[derive(Deserialize, Debug)]
struct Message {
    content: Value,
}

impl OcrClient {
    pub fn new(api_key: String, endpoint: String, model: String, prompt: String) -> Self {
        Self {
            api_key,
            endpoint,
            model,
            prompt,
            client: Client::new(),
        }
    }

    fn log_to_history(&self, tag: &str, data: &str) {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(HISTORY_PATH)
        {
            let _ = writeln!(
                f,
                "[{}] [{}] {}",
                Local::now().format("%H:%M:%S"),
                tag,
                data
            );
        }
    }

    fn generate_payload(&self, b64: &str) -> Value {
        json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": self.prompt},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
                ]
            }],
            "response_format": { "type": "json_object" },
            "temperature": 0.1
        })
    }

    pub fn call_api(
        &self,
        b64: &str,
    ) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        let payload = self.generate_payload(b64);

        let res = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/HidekiAI/lenzu")
            .json(&payload)
            .send()?;

        let status = res.status();
        let raw_response = res.text()?;
        self.log_to_history("RAW_API_RESPONSE", &raw_response);

        if !status.is_success() {
            let snippet: String = raw_response.chars().take(800).collect();
            return Err(format!("OpenRouter HTTP {} — {}", status, snippet).into());
        }

        let response_data: OpenRouterResponse = serde_json::from_str(&raw_response).map_err(|e| {
            let snippet: String = raw_response.chars().take(400).collect();
            format!("Invalid API JSON ({}): {}", e, snippet)
        })?;

        let content_value = &response_data
            .choices
            .get(0)
            .ok_or("No choices in API response")?
            .message
            .content;

        // Normalize the shape: always returns a Vec
        self.normalize_results(content_value)
    }

    fn normalize_results(
        &self,
        val: &Value,
    ) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        // 1. Unwrap stringified JSON if necessary
        let actual_json = if val.is_string() {
            serde_json::from_str(val.as_str().unwrap())?
        } else {
            val.clone()
        };

        // 2. Handle array, or object (json_object mode often wraps the array in a key).
        if actual_json.is_array() {
            let results: Vec<TranslationResult> = serde_json::from_value(actual_json)?;
            return Ok(results);
        }

        if let Some(obj) = actual_json.as_object() {
            const WRAPPER_KEYS: &[&str] = &[
                "results",
                "items",
                "data",
                "translations",
                "lines",
                "ocr",
                "bubbles",
                "detections",
                "text_blocks",
            ];
            for key in WRAPPER_KEYS {
                if let Some(Value::Array(arr)) = obj.get(*key) {
                    let results: Vec<TranslationResult> =
                        serde_json::from_value(Value::Array(arr.clone()))?;
                    return Ok(results);
                }
            }

            let single: TranslationResult = serde_json::from_value(Value::Object(obj.clone()))?;
            return Ok(vec![single]);
        }

        Err("Unexpected JSON shape (neither object nor array)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalization_handles_single_object() {
        let client = OcrClient::new("key".into(), String::new(), String::new(), String::new());
        let obj = json!({"original": "single", "english": "one"});
        let results = client.normalize_results(&obj).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "single");
    }

    #[test]
    fn test_normalization_handles_array() {
        let client = OcrClient::new("key".into(), String::new(), String::new(), String::new());
        let arr = json!([
            {"original": "line1", "english": "one"},
            {"original": "line2", "english": "two"}
        ]);
        let results = client.normalize_results(&arr).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].original, "line2");
    }

    #[test]
    fn test_normalization_handles_json_object_wrapper() {
        let client = OcrClient::new("key".into(), String::new(), String::new(), String::new());
        let wrapped = json!({
            "results": [
                {"original": "a", "english": "A"},
                {"original": "b", "english": "B"}
            ]
        });
        let results = client.normalize_results(&wrapped).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].english.as_deref(), Some("A"));
    }
}
