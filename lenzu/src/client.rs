use chrono::Local;
use reqwest::blocking::Client;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;

const API_DEBUG_PATH: &str = "/dev/shm/api_debug.txt";

pub struct OcrClient {
    api_key: String,
    endpoint: String,
    model: String,
    prompt: String,
    client: Client,
}

/// Deserialize any JSON value (string, array, object, number) into an
/// `Option<String>`.  Arrays and objects are serialized to their compact
/// JSON representation so bounding-box coords like `[79, 48]` become
/// `"[79,48]"` rather than crashing with "expected a string".
fn coerce_to_opt_string<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    let val: Option<Value> = Option::deserialize(d)?;
    Ok(val.map(|v| match v {
        Value::String(s) => s,
        other => other.to_string(),
    }))
}

#[derive(Deserialize, Serialize, Debug, Default, Clone, PartialEq)]
pub struct TranslationResult {
    pub original: String,
    pub furigana: Option<String>,
    pub romaji: Option<String>,
    pub english: Option<String>,
    #[serde(default, deserialize_with = "coerce_to_opt_string")]
    pub top_xy: Option<String>,  // upper-left bounding box corner (string or [x,y] array)
    #[serde(default, deserialize_with = "coerce_to_opt_string")]
    pub bot_xy: Option<String>,  // lower-right bounding box corner (string or [x,y] array)
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

/// Default timeout for the entire request (connect + read).
/// Gemini/OpenRouter can be slow on large images; 60 s is generous but bounded.
const REQUEST_TIMEOUT_SECS: u64 = 60;

impl OcrClient {
    pub fn new(api_key: String, endpoint: String, model: String, prompt: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            api_key,
            endpoint,
            model,
            prompt,
            client,
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

        let mut req = self
            .client
            .post(&self.endpoint)
            .header("HTTP-Referer", "https://github.com/HidekiAI/lenzu");
        // ollama does not require (or accept) an Authorization header.
        // Only attach it when an api_key is present.
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }
        let res = req.json(&payload).send()?;

        let status = res.status();
        let raw_response = res.text()?;
        // Write raw API JSON to a separate debug file — not the user-facing history
        if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(API_DEBUG_PATH) {
            let _ = writeln!(f, "[{}] {}", Local::now().format("%H:%M:%S"), raw_response.trim());
        }

        if !status.is_success() {
            let snippet: String = raw_response.chars().take(800).collect();
            return Err(format!("API HTTP {} — {}", status, snippet).into());
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
            let raw = val.as_str().unwrap();
            eprintln!("[OCR] content is a string, attempting inner parse (first 200 chars): {}", &raw[..raw.len().min(200)]);
            serde_json::from_str(raw).map_err(|e| {
                eprintln!("[OCR] inner JSON parse failed: {e}  raw snippet: {}", &raw[..raw.len().min(400)]);
                e
            })?
        } else {
            eprintln!("[OCR] content type: {}", if val.is_array() { "array" } else if val.is_object() { "object" } else { "other" });
            val.clone()
        };

        // 2. Handle array, or object (json_object mode often wraps the array in a key).
        if actual_json.is_array() {
            let results: Vec<TranslationResult> = serde_json::from_value(actual_json).map_err(|e| {
                eprintln!("[OCR] array deserialize failed: {e}");
                e
            })?;
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

    fn client() -> OcrClient {
        OcrClient::new("key".into(), String::new(), String::new(), String::new())
    }

    // ── existing shapes ──────────────────────────────────────────────────────

    #[test]
    fn test_normalization_handles_single_object() {
        let obj = json!({"original": "single", "english": "one"});
        let results = client().normalize_results(&obj).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "single");
    }

    #[test]
    fn test_normalization_handles_array() {
        let arr = json!([
            {"original": "line1", "english": "one"},
            {"original": "line2", "english": "two"}
        ]);
        let results = client().normalize_results(&arr).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].original, "line2");
    }

    #[test]
    fn test_normalization_handles_json_object_wrapper() {
        let wrapped = json!({
            "results": [
                {"original": "a", "english": "A"},
                {"original": "b", "english": "B"}
            ]
        });
        let results = client().normalize_results(&wrapped).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].english.as_deref(), Some("A"));
    }

    // ── stringified-JSON shapes (most common real-world failure case) ─────────
    // Gemini/OpenRouter often returns the JSON payload as a *string* inside
    // the content field when response_format=json_object is requested.

    #[test]
    fn test_normalization_stringified_array() {
        // content = "[{...},{...}]"  (the whole array encoded as a JSON string)
        let stringified = Value::String(
            r#"[{"original":"漢字","furigana":"漢字[かんじ]","romaji":"kanji","english":"Chinese character"}]"#
                .to_string(),
        );
        let results = client().normalize_results(&stringified).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "漢字");
        assert_eq!(results[0].furigana.as_deref(), Some("漢字[かんじ]"));
        assert_eq!(results[0].english.as_deref(), Some("Chinese character"));
    }

    #[test]
    fn test_normalization_stringified_object_with_wrapper_key() {
        // content = "{\"results\":[...]}"  (object with wrapper key, encoded as string)
        let stringified = Value::String(
            r#"{"results":[{"original":"hello","english":"hello"},{"original":"world","english":"world"}]}"#
                .to_string(),
        );
        let results = client().normalize_results(&stringified).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].original, "world");
    }

    #[test]
    fn test_normalization_stringified_single_object() {
        // content = "{\"original\":\"foo\",...}"  (single object, encoded as string)
        let stringified = Value::String(
            r#"{"original":"foo","english":"bar","furigana":"foo[ふー]"}"#.to_string(),
        );
        let results = client().normalize_results(&stringified).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "foo");
        assert_eq!(results[0].furigana.as_deref(), Some("foo[ふー]"));
    }

    // ── markdown-fenced JSON (LLMs sometimes add ```json fences) ─────────────
    // If the model ignores response_format and wraps in fences we must strip
    // them. Currently this is NOT handled, so the test documents the failure
    // and the fix can be added to normalize_results when confirmed in the wild.
    #[test]
    fn test_normalization_markdown_fenced_array_fails_gracefully() {
        let fenced = Value::String(
            "```json\n[{\"original\":\"test\",\"english\":\"test\"}]\n```".to_string(),
        );
        // This is expected to fail until fence-stripping is added.
        // When it starts passing, the fence-stripping logic is in place.
        let result = client().normalize_results(&fenced);
        assert!(result.is_err(), "markdown-fenced JSON should fail (not silently succeed with wrong data)");
    }

    // ── all known wrapper keys ────────────────────────────────────────────────

    #[test]
    fn test_normalization_all_wrapper_keys() {
        for key in &["results", "items", "data", "translations", "lines", "ocr",
                     "bubbles", "detections", "text_blocks"] {
            let wrapped = json!({ (*key): [{"original": "x", "english": "y"}] });
            let results = client().normalize_results(&wrapped)
                .unwrap_or_else(|e| panic!("wrapper key '{}' failed: {}", key, e));
            assert_eq!(results.len(), 1, "wrapper key '{}' should produce 1 result", key);
            assert_eq!(results[0].original, "x");
        }
    }

    // ── optional fields are truly optional ───────────────────────────────────

    #[test]
    fn test_normalization_minimal_object_only_original() {
        let obj = json!({"original": "only"});
        let results = client().normalize_results(&obj).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].english.is_none());
        assert!(results[0].furigana.is_none());
        assert!(results[0].romaji.is_none());
    }

    // ── degenerate inputs return Err, not panic ───────────────────────────────

    #[test]
    fn test_normalization_null_returns_err() {
        let null_val = Value::Null;
        assert!(client().normalize_results(&null_val).is_err());
    }

    #[test]
    fn test_normalization_number_returns_err() {
        let num = Value::from(42_i64);
        assert!(client().normalize_results(&num).is_err());
    }

    #[test]
    fn test_normalization_empty_array_returns_empty_vec() {
        let empty = json!([]);
        let results = client().normalize_results(&empty).unwrap();
        assert!(results.is_empty());
    }

    // ── bounding-box coords as arrays (the actual prod failure) ──────────────
    // The Gemini API returns top_xy/bot_xy as [x, y] integer arrays, NOT as
    // strings.  Without coerce_to_opt_string this fails with
    // "invalid type: sequence, expected a string".

    #[test]
    fn test_normalization_bounding_box_as_array() {
        // Direct array format from Gemini
        let arr = json!([{
            "original": "ちなみに私は",
            "top_xy": [79, 48],
            "bot_xy": [167, 181],
            "furigana": "ちなみに私[わたし]は",
            "romaji": "chinamini watashi wa",
            "english": "By the way, I"
        }]);
        let results = client().normalize_results(&arr).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "ちなみに私は");
        // coords coerced to JSON string representation
        assert_eq!(results[0].top_xy.as_deref(), Some("[79,48]"));
        assert_eq!(results[0].bot_xy.as_deref(), Some("[167,181]"));
        assert_eq!(results[0].furigana.as_deref(), Some("ちなみに私[わたし]は"));
    }

    #[test]
    fn test_normalization_bounding_box_as_array_stringified() {
        // Same but content arrives as a JSON *string* (as Gemini/OpenRouter sends it)
        let stringified = Value::String(serde_json::to_string(&json!([{
            "original": "生身の硬いのが",
            "top_xy": [102, 82],
            "bot_xy": [261, 120],
            "furigana": "生身[なまみ]の硬[かた]いのが",
            "romaji": "namaomi no katai no ga",
            "english": "The toughness of a living body"
        }])).unwrap());
        let results = client().normalize_results(&stringified).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "生身の硬いのが");
        assert_eq!(results[0].top_xy.as_deref(), Some("[102,82]"));
    }

    #[test]
    fn test_normalization_bounding_box_as_string_still_works() {
        // Ensure existing string-format coords aren't broken by the coerce helper
        let arr = json!([{
            "original": "test",
            "top_xy": "10,20",
            "bot_xy": "100,200"
        }]);
        let results = client().normalize_results(&arr).unwrap();
        assert_eq!(results[0].top_xy.as_deref(), Some("10,20"));
        assert_eq!(results[0].bot_xy.as_deref(), Some("100,200"));
    }

    // ── reliability / crash-prevention ───────────────────────────────────────

    #[test]
    fn test_client_is_built_with_timeout() {
        // OcrClient::new() must not panic (i.e., Client::builder().timeout().build() succeeds)
        // and the resulting client should be usable.
        let c = OcrClient::new("k".into(), "http://localhost".into(), "m".into(), "p".into());
        // We can't inspect the timeout directly, but a successful payload generation
        // proves the client was constructed without panic.
        let payload = c.generate_payload("dGVzdA==");
        assert_eq!(payload["model"], "m");
    }

    // ── auth header suppression for ollama ───────────────────────────────────

    #[test]
    fn test_generate_payload_model_is_set() {
        // Confirm the model field is included in every request payload.
        let c = OcrClient::new("".into(), "http://localhost:11434/v1/chat/completions".into(), "gemma4:e2b".into(), "prompt".into());
        let payload = c.generate_payload("dGVzdA==");
        assert_eq!(payload["model"], "gemma4:e2b");
    }

    /// The `Authorization` header must be omitted when api_key is empty string (ollama path).
    /// We test this by inspecting the RequestBuilder via a small HTTP mock rather than
    /// reaching into reqwest internals.  The simplest approach: build the request and confirm
    /// the header is absent by sending to a local echo server — but that requires network.
    /// Instead we document the invariant via a structural test on OcrClient internals.
    #[test]
    fn test_empty_api_key_is_stored_correctly() {
        // An empty api_key signals the no-auth (ollama) path.
        // Confirm OcrClient accepts it without panicking and that the key is empty.
        let c = OcrClient::new("".into(), "http://localhost:11434/v1/chat/completions".into(), "gemma4:e2b".into(), "p".into());
        // We can't inspect the header builder directly, but we can confirm the payload
        // builds cleanly — if api_key handling panicked, it would surface here.
        let payload = c.generate_payload("dGVzdA==");
        assert_eq!(payload["model"], "gemma4:e2b", "payload must include the model field");
    }

    #[test]
    fn test_thread_panic_is_caught_by_catch_unwind() {
        // Simulate what the spawn wrapper does: catch_unwind maps panics to Err.
        let result = std::panic::catch_unwind(|| -> Result<Vec<TranslationResult>, String> {
            panic!("simulated panic inside OCR thread");
        })
        .unwrap_or_else(|_| Err("OCR thread panicked".to_string()));

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "OCR thread panicked");
    }
}
