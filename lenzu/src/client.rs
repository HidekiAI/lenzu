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

        let raw_response = res.text()?;
        self.log_to_history("RAW_API_RESPONSE", &raw_response);

        let response_data: OpenRouterResponse = serde_json::from_str(&raw_response)?;

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

        // 2. Handle the "Object vs Array" fallback
        if actual_json.is_array() {
            // Case: [{}, {}]
            let results: Vec<TranslationResult> = serde_json::from_value(actual_json)?;
            Ok(results)
        } else if actual_json.is_object() {
            // Case: {} -> wrap in Vec
            let single: TranslationResult = serde_json::from_value(actual_json)?;
            Ok(vec![single])
        } else {
            Err("Unexpected JSON shape (neither object nor array)".into())
        }
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
}
