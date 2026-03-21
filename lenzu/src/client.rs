use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct OcrClient {
    api_key: String,
    client: Client,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct TranslationResult {
    pub original: String,
    pub furigana: Option<String>,
    pub romaji: Option<String>,
    pub english: Option<String>,
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
    content: String,
}

impl OcrClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    /// Sends the base64 image to OpenRouter and requests a structured JSON response.
    pub fn call_api(&self, b64: &str) -> Result<TranslationResult, Box<dyn std::error::Error>> {
        // The prompt is engineered to force a JSON-only response for easier parsing.
        let prompt = "OCR the Japanese text in this image. \
                      Return ONLY a JSON object with these keys: \
                      'original' (raw OCR text), \
                      'furigana' (kanji with [reading], e.g. 漢字[かんじ]), \
                      'romaji' (latin script), \
                      'english' (translation). \
                      If the image is blurry or contains no Japanese, provide a 'debug_info' \
                      explaining why in italics and leave other fields empty.";

        // Note: I also use FREE:
        // - endpoint = "https://openrouter.ai/api/v1",
        // - model = "openrouter/free",
        // It is rate-limited, but if you have paid openrouter account, their limit is increased
        let res = self.client.post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://github.com/HidekiAI/lenzu")
            .json(&json!({
                "model": "google/gemini-2.0-flash-001", // is it cheaper to use DeepSeek?
                "messages": [{
                    "role": "user", 
                    "content": [
                        {"type": "text", "text": prompt},
                        {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
                    ]
                }],
                "response_format": { "type": "json_object" }
            })).send()?;

        let response_data: OpenRouterResponse = res.json()?;

        // Extract the raw string content from the LLM response
        let content_raw = response_data
            .choices
            .get(0)
            .map(|c| c.message.content.trim())
            .ok_or("Empty response from API")?;

        // Parse the inner JSON string into our TranslationResult struct
        let result: TranslationResult = serde_json::from_str(content_raw)?;

        Ok(result)
    }
}
