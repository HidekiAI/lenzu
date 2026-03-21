use reqwest::blocking::Client;
use serde_json::json;

pub struct OcrClient {
    api_key: String,
    client: Client,
}

impl OcrClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    pub fn call_api(&self, b64: &str) -> Result<String, Box<dyn std::error::Error>> {
        let res = self.client.post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/HidekiAI/lenzu")
            .json(&json!({
                "model": "google/gemini-2.0-flash-001",
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "OCR the Japanese text. Output transcription only, line by line."},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
                ]}]
            })).send()?;

        let body: serde_json::Value = res.json()?;

        Ok(body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string())
    }
}
