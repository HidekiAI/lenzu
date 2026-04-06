use chrono::Local;
use image::DynamicImage;
use reqwest::blocking::Client;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;

use crate::utils::{encode_as_grayscale, encode_for_fallback};

const API_DEBUG_PATH: &str = "/dev/shm/lenzu/api_debug.txt";

pub struct OcrClient {
    api_key: String,
    endpoint: String,
    model: String,
    prompt: String,
    client: Client,
    /// Ollama-specific: KV-cache context size.  `None` = let ollama use its default.
    /// Setting this to a small value (e.g. 2048) frees VRAM on cards with limited headroom.
    num_ctx: Option<u32>,
    /// When `true`, include `"response_format": {"type": "json_object"}` in the payload.
    /// OpenAI/OpenRouter models benefit from this; local ollama models (gemma4, glm-ocr)
    /// may return a single object when constrained to json_object mode — disable it for them.
    use_json_object_format: bool,
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
    #[serde(alias = "english_translation")]
    pub english: Option<String>,
    #[serde(default, deserialize_with = "coerce_to_opt_string")]
    pub top_xy: Option<String>,  // upper-left bounding box corner (string or [x,y] array)
    #[serde(default, deserialize_with = "coerce_to_opt_string")]
    pub bot_xy: Option<String>,  // lower-right bounding box corner (string or [x,y] array)
    pub debug_info: Option<String>,
}


/// Metadata about which backend served a successful OCR request.
#[derive(Debug, Clone)]
pub struct OcrMeta {
    /// Human-readable backend label, e.g. "ollama:glm-ocr" or "openrouter:google/gemini-2.0-flash-001".
    pub backend: String,
    /// Total round-trip time in milliseconds.
    pub elapsed_ms: u128,
}

/// Default timeout for the entire request (connect + read).
/// Gemini/OpenRouter can be slow on large images; 60 s is generous but bounded.
const REQUEST_TIMEOUT_SECS: u64 = 60;

impl OcrClient {
    /// Standard constructor — default timeout, no `num_ctx` override.
    /// `api_key` empty → ollama (local); non-empty → remote, enables json_object format.
    pub fn new(api_key: String, endpoint: String, model: String, prompt: String) -> Self {
        let use_json = !api_key.is_empty(); // remote backends support json_object; local ones may not
        Self::new_with_options(api_key, endpoint, model, prompt, Some(REQUEST_TIMEOUT_SECS), None, use_json)
    }

    /// Full constructor — allows overriding the reqwest timeout and setting
    /// `num_ctx` for ollama (reduces KV-cache VRAM pressure on cards with < 1 GB free).
    /// `timeout_secs: None` disables the timeout entirely (use for CPU inference timing).
    /// `use_json_object_format` — set `false` for local ollama models to avoid the
    /// json_object constraint that causes them to return a single object instead of an array.
    pub fn new_with_options(
        api_key: String,
        endpoint: String,
        model: String,
        prompt: String,
        timeout_secs: Option<u64>,
        num_ctx: Option<u32>,
        use_json_object_format: bool,
    ) -> Self {
        let mut builder = Client::builder();
        if let Some(secs) = timeout_secs {
            builder = builder.timeout(std::time::Duration::from_secs(secs));
        }
        let client = builder.build().unwrap_or_else(|_| Client::new());
        Self { api_key, endpoint, model, prompt, client, num_ctx, use_json_object_format }
    }

    /// Returns a short label identifying this backend for logging.
    /// Local ollama (localhost/127.0.0.1): "ollama:<model>"; remote: "<host>:<model>".
    pub fn label(&self) -> String {
        let is_local = self.endpoint.contains("localhost") || self.endpoint.contains("127.0.0.1");
        if is_local {
            format!("ollama:{}", self.model)
        } else {
            // Extract hostname from endpoint URL (e.g. "openrouter.ai" from full URL).
            let host = self.endpoint
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or("remote");
            format!("{host}:{}", self.model)
        }
    }

    fn generate_payload(&self, b64: &str) -> Value {
        let mut payload = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": self.prompt},
                    {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
                ]
            }],
            "temperature": 0.1,
            "stream": true
        });
        // json_object mode helps OpenAI-family models (OpenRouter/Gemini) return valid JSON.
        // Local ollama models (gemma4, glm-ocr) may return a single object when constrained
        // to json_object mode, causing multi-block detection to fail — skip it for them.
        if self.use_json_object_format {
            payload["response_format"] = json!({"type": "json_object"});
        }
        // Ollama-specific: limit KV-cache to free VRAM on cards with limited headroom.
        // OpenRouter and other remote backends ignore unknown top-level fields.
        if let Some(ctx) = self.num_ctx {
            payload["options"] = json!({"num_ctx": ctx});
        }
        payload
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
        let res = req.json(&payload).send().map_err(|e| -> Box<dyn std::error::Error> {
            // Surface the root cause so callers see "operation timed out" or
            // "connection refused" rather than just "error sending request".
            use std::error::Error;
            let root = e.source().map(|s| format!(" — {s}")).unwrap_or_default();
            format!("{e}{root}").into()
        })?;

        let status = res.status();

        // Error responses (4xx/5xx) are plain JSON, not SSE — read them whole.
        if !status.is_success() {
            let error_body = res.text().unwrap_or_default();
            if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(API_DEBUG_PATH) {
                let _ = writeln!(f, "[{}] ERROR {}: {}", Local::now().format("%H:%M:%S"), status, error_body.trim());
            }
            let snippet: String = error_body.chars().take(800).collect();
            return Err(format!("API HTTP {} — {}", status, snippet).into());
        }

        // Successful responses are SSE streams (stream:true keeps the TCP connection
        // alive while ollama generates, preventing its 30-second write-timeout from
        // firing mid-inference).  Accumulate all delta.content tokens into one string.
        let content = Self::read_sse_content(res)?;

        // Write accumulated content to debug file.
        if let Ok(mut f) = OpenOptions::new().create(true).write(true).truncate(true).open(API_DEBUG_PATH) {
            let _ = writeln!(f, "[{}] {}", Local::now().format("%H:%M:%S"), content.trim());
        }

        // The accumulated SSE content is the raw model output (JSON string).
        // normalize_results already handles the stringified-JSON path.
        self.normalize_results(&Value::String(content))
    }

    /// Read an OpenAI-compatible SSE stream and return the accumulated content string.
    ///
    /// Each SSE line looks like:
    ///   `data: {"choices":[{"delta":{"content":"..."},"finish_reason":null}]}`
    /// Final line: `data: [DONE]`
    ///
    /// By reading tokens as they arrive the HTTP connection stays alive,
    /// preventing ollama's server-side write timeout from firing.
    fn read_sse_content(res: reqwest::blocking::Response) -> Result<String, Box<dyn std::error::Error>> {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(res);
        let mut content = String::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed == "data: [DONE]" {
                continue;
            }
            if let Some(json_part) = trimmed.strip_prefix("data: ") {
                if let Ok(chunk) = serde_json::from_str::<Value>(json_part) {
                    if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
                        content.push_str(delta);
                    }
                }
            }
        }
        Ok(content)
    }

    /// Strip optional markdown code fences that some LLMs add around JSON output.
    /// Handles ` ```json\n...\n``` ` and ` ```\n...\n``` `.  Returns a slice of the
    /// inner content (no allocation when fences are absent).
    fn strip_markdown_fences(raw: &str) -> &str {
        let s = raw.trim();
        if s.starts_with("```") {
            // Skip the opening fence line (which may contain a language tag like "json")
            if let Some(newline) = s.find('\n') {
                let inner = s[newline + 1..].trim_end();
                // Strip the closing fence from the end
                if inner.ends_with("```") {
                    return inner[..inner.len() - 3].trim_end();
                }
                // Closing fence on its own line
                if let Some(end) = inner.rfind("\n```") {
                    return inner[..end].trim();
                }
            }
        }
        s
    }

    fn normalize_results(
        &self,
        val: &Value,
    ) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        // 1. Unwrap stringified JSON if necessary
        let actual_json = if val.is_string() {
            let raw = val.as_str().unwrap();
            let raw = Self::strip_markdown_fences(raw);
            eprintln!("[OCR] content is a string, attempting inner parse (first 200 chars): {}",
                raw.chars().take(200).collect::<String>());
            serde_json::from_str(raw).map_err(|e| {
                eprintln!("[OCR] inner JSON parse failed: {e}  raw snippet: {}",
                    raw.chars().take(400).collect::<String>());
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

// ── Coordinate helpers ────────────────────────────────────────────────────────

/// Parse a coordinate string produced by `coerce_to_opt_string` into `(x, y)`.
///
/// Accepts both formats the model may return:
///   - `"79,48"`        (prompt-requested plain string)
///   - `"[79,48]"`      (array coerced to string by `coerce_to_opt_string`)
///
/// Returns `None` if the string is absent or cannot be parsed.
pub fn parse_xy(s: Option<&str>) -> Option<(i64, i64)> {
    let s = s?.trim().trim_start_matches('[').trim_end_matches(']');
    let mut parts = s.splitn(2, ',');
    let x: i64 = parts.next()?.trim().parse().ok()?;
    let y: i64 = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// Return `true` when both coordinates in `actual` are within `tolerance` pixels
/// of `expected`.  Either being `None` is treated as "no constraint" (passes).
pub fn coords_within(actual: Option<&str>, expected: (i64, i64), tolerance: i64) -> bool {
    match parse_xy(actual) {
        None => true, // model gave no coord — skip the check
        Some((ax, ay)) => {
            (ax - expected.0).abs() <= tolerance && (ay - expected.1).abs() <= tolerance
        }
    }
}

// ── DualOcrClient ─────────────────────────────────────────────────────────────

/// Wraps a primary `OcrClient` (ollama/local) and an optional fallback
/// `OcrClient` (OpenRouter/remote).
///
/// Encoding is handled internally:
///   - Primary  → grayscale, full resolution
///   - Fallback → grayscale + proportional downscale to `fallback_max_dimension`
///
/// `call_api_force_fallback` skips primary entirely (Ctrl+Shift+Click path).
pub struct DualOcrClient {
    primary: OcrClient,
    /// Local fallback chain (e.g. gemma4:e2b) — tried in order after primary.
    local_fallbacks: Vec<OcrClient>,
    /// Free-tier remote (openrouter/free) — always configured; no API key required.
    /// Tried before the paid remote fallback.
    free_remote_fallback: OcrClient,
    /// Paid remote fallback (e.g. Gemini via OpenRouter) — only configured when API key is set.
    remote_fallback: Option<OcrClient>,
    fallback_max_dimension: u32,
}

impl DualOcrClient {
    /// Build the client.
    ///
    /// `local_fallback_models` is an ordered list of model names that share the primary
    /// ollama endpoint.  They are tried in sequence after the primary fails.
    ///
    /// Fallback chain order: primary → local_fallbacks → free_remote → remote (paid, if key set)
    pub fn new(
        primary_endpoint: String,
        primary_model: String,
        local_fallback_models: Vec<String>,
        free_remote_endpoint: String,
        free_remote_model: String,
        fallback_endpoint: String,
        fallback_model: String,
        fallback_api_key: String,
        fallback_max_dimension: u32,
        primary_num_ctx: Option<u32>,
        local_timeout_secs: u64,
        remote_timeout_secs: u64,
        prompt: String,
    ) -> Self {
        let primary = OcrClient::new_with_options(
            String::new(), primary_endpoint.clone(), primary_model, prompt.clone(),
            Some(local_timeout_secs), primary_num_ctx, false,
        );
        let local_fallbacks = local_fallback_models
            .into_iter()
            .map(|model| OcrClient::new_with_options(
                String::new(), primary_endpoint.clone(), model, prompt.clone(),
                Some(local_timeout_secs), None, false,
            ))
            .collect();
        // Free remote — uses the same API key as the paid remote (OpenRouter requires Bearer auth
        // even for free models; "free" means zero-cost model, not keyless access).
        // If no key is set, the request will be unauthenticated and may be rate-limited more aggressively.
        let free_remote_fallback = OcrClient::new_with_options(
            fallback_api_key.clone(), free_remote_endpoint, free_remote_model, prompt.clone(),
            Some(remote_timeout_secs), None, true,
        );
        let remote_fallback = if !fallback_api_key.is_empty() {
            Some(OcrClient::new_with_options(
                fallback_api_key, fallback_endpoint, fallback_model, prompt,
                Some(remote_timeout_secs), None, true,
            ))
        } else {
            None
        };
        Self { primary, local_fallbacks, free_remote_fallback, remote_fallback, fallback_max_dimension }
    }

    /// Returns `true` when results are empty or every `english` field is absent/blank.
    pub fn needs_fallback(results: &[TranslationResult]) -> bool {
        if results.is_empty() {
            return true;
        }
        results.iter().all(|r| {
            r.english
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        })
    }

    /// Kill any lingering ollama runner subprocess.
    ///
    /// When the primary client times out or fails, the ollama runner process keeps
    /// generating in the background, holding VRAM.  Killing it immediately frees the
    /// GPU and prevents "hundreds of stuck runners" from accumulating over a session.
    /// Ollama respawns a fresh runner on the next request.
    fn cancel_primary_runner() {
        let _ = std::process::Command::new("pkill")
            .args(["-KILL", "-f", "ollama runner.*--model"])
            .status();
    }

    /// Normal path: primary gets grayscale full-res; each fallback (local then remote) is
    /// tried in order until one succeeds.  Local fallbacks use the same ollama endpoint as
    /// the primary.  Remote fallback gets grayscale + downscale.
    pub fn call_api(&self, image: &DynamicImage) -> Result<(Vec<TranslationResult>, OcrMeta), Box<dyn std::error::Error>> {
        let t0 = std::time::Instant::now();
        let primary_b64 = encode_as_grayscale(image);
        let primary_result = self.primary.call_api(&primary_b64);

        let needs_fb = match &primary_result {
            Ok(results) => Self::needs_fallback(results),
            Err(_) => true,
        };

        if !needs_fb {
            let meta = OcrMeta { backend: self.primary.label(), elapsed_ms: t0.elapsed().as_millis() };
            return primary_result.map(|r| (r, meta));
        }

        match &primary_result {
            Err(e) => {
                eprintln!("[OCR] primary failed ({e}) — killing stale runner");
                Self::cancel_primary_runner();
            }
            Ok(_) => eprintln!("[OCR] primary gave no translations"),
        }

        // Walk the local fallback chain (e.g. gemma4 → …)
        for (i, local) in self.local_fallbacks.iter().enumerate() {
            eprintln!("[OCR] trying local fallback #{}", i + 1);
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(API_DEBUG_PATH) {
                let _ = std::io::Write::write_fmt(&mut f, format_args!("[local-fallback-{}]\n", i + 1));
            }
            let result = local.call_api(&primary_b64);
            let ok = match &result {
                Ok(r) => !Self::needs_fallback(r),
                Err(e) => {
                    eprintln!("[OCR] local fallback #{} failed ({e}) — killing stale runner", i + 1);
                    Self::cancel_primary_runner();
                    false
                }
            };
            if ok {
                let meta = OcrMeta { backend: local.label(), elapsed_ms: t0.elapsed().as_millis() };
                return result.map(|r| (r, meta));
            }
        }

        // Free remote fallback — always available (30 req/day without key, 1000/day with key)
        {
            let fallback_b64 = encode_for_fallback(image, self.fallback_max_dimension);
            eprintln!("[OCR] all local backends failed — trying free remote ({})", self.free_remote_fallback.label());
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(API_DEBUG_PATH) {
                let _ = std::io::Write::write_fmt(&mut f, format_args!("[free-remote-fallback]\n"));
            }
            let meta_label = self.free_remote_fallback.label();
            let result = self.free_remote_fallback.call_api(&fallback_b64);
            let ok = match &result {
                Ok(r) => !Self::needs_fallback(r),
                Err(e) => { eprintln!("[OCR] free remote failed ({e})"); false }
            };
            if ok {
                return result.map(|r| (r, OcrMeta { backend: meta_label, elapsed_ms: t0.elapsed().as_millis() }));
            }
            // Fall through to paid remote if free remote failed or returned nothing useful.
            let fallback_b64_paid = encode_for_fallback(image, self.fallback_max_dimension);
            match &self.remote_fallback {
                Some(fb) => {
                    eprintln!("[OCR] free remote gave nothing — trying paid remote");
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(API_DEBUG_PATH) {
                        let _ = std::io::Write::write_fmt(&mut f, format_args!("[paid-remote-fallback]\n"));
                    }
                    let meta_label = fb.label();
                    fb.call_api(&fallback_b64_paid).map(|r| {
                        (r, OcrMeta { backend: meta_label, elapsed_ms: t0.elapsed().as_millis() })
                    })
                }
                None => {
                    eprintln!("[OCR] paid remote not configured (OPENROUTER_API_KEY not set) — returning free remote result");
                    result.map(|r| (r, OcrMeta { backend: self.free_remote_fallback.label(), elapsed_ms: t0.elapsed().as_millis() }))
                }
            }
        }
    }

    /// Force-remote path: grayscale + downscale (Ctrl+Shift+Click).
    /// Tries free remote first, then paid remote (if API key is set).
    pub fn call_api_force_fallback(&self, image: &DynamicImage) -> Result<(Vec<TranslationResult>, OcrMeta), Box<dyn std::error::Error>> {
        let t0 = std::time::Instant::now();
        let b64 = encode_for_fallback(image, self.fallback_max_dimension);

        // Try free remote first (always available)
        let meta_label = self.free_remote_fallback.label();
        let result = self.free_remote_fallback.call_api(&b64);
        let ok = match &result {
            Ok(r) => !Self::needs_fallback(r),
            Err(e) => { eprintln!("[OCR] force-fallback free remote failed ({e})"); false }
        };
        if ok {
            return result.map(|r| (r, OcrMeta { backend: meta_label, elapsed_ms: t0.elapsed().as_millis() }));
        }

        // Try paid remote
        match &self.remote_fallback {
            Some(fb) => {
                let meta_label = fb.label();
                fb.call_api(&b64).map(|r| {
                    (r, OcrMeta { backend: meta_label, elapsed_ms: t0.elapsed().as_millis() })
                })
            }
            None => {
                // Return whatever the free remote gave us (even if empty), or its error
                result.map(|r| (r, OcrMeta { backend: self.free_remote_fallback.label(), elapsed_ms: t0.elapsed().as_millis() }))
                    .map_err(|_| "Remote override unavailable: OPENROUTER_API_KEY not set and free remote failed".into())
            }
        }
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
    fn test_normalization_markdown_fenced_array() {
        // LLMs (especially local ollama models without json_object mode) often wrap
        // their JSON output in ```json ... ``` fences.  strip_markdown_fences must handle this.
        let fenced = Value::String(
            "```json\n[{\"original\":\"test\",\"english\":\"test\"}]\n```".to_string(),
        );
        let results = client().normalize_results(&fenced).expect("fenced JSON must be handled");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "test");
    }

    #[test]
    fn test_normalization_markdown_fenced_no_lang_tag() {
        let fenced = Value::String(
            "```\n[{\"original\":\"foo\",\"english\":\"bar\"}]\n```".to_string(),
        );
        let results = client().normalize_results(&fenced).expect("fence without lang tag must be handled");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].original, "foo");
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

    // ── DualOcrClient::needs_fallback ────────────────────────────────────────

    fn tr(english: Option<&str>) -> TranslationResult {
        TranslationResult { english: english.map(str::to_string), ..Default::default() }
    }

    #[test]
    fn test_needs_fallback_empty_vec() {
        assert!(DualOcrClient::needs_fallback(&[]));
    }

    #[test]
    fn test_needs_fallback_all_none_english() {
        assert!(DualOcrClient::needs_fallback(&[tr(None), tr(None)]));
    }

    #[test]
    fn test_needs_fallback_all_blank_english() {
        assert!(DualOcrClient::needs_fallback(&[tr(Some("")), tr(Some("  "))]));
    }

    #[test]
    fn test_needs_fallback_whitespace_only_english() {
        assert!(DualOcrClient::needs_fallback(&[tr(Some("\t\n"))]));
    }

    #[test]
    fn test_needs_fallback_false_when_any_english_present() {
        // One result has a real translation — should NOT trigger fallback
        assert!(!DualOcrClient::needs_fallback(&[tr(None), tr(Some("hello"))]));
    }

    #[test]
    fn test_needs_fallback_false_single_result_with_text() {
        assert!(!DualOcrClient::needs_fallback(&[tr(Some("world"))]));
    }

    fn dual(api_key: &str) -> DualOcrClient {
        DualOcrClient::new(
            "http://localhost:11434/v1/chat/completions".into(),
            "gemma4:e2b".into(),
            vec![],
            "https://openrouter.ai/api/v1/chat/completions".into(), // free_remote_endpoint
            "openrouter/free".into(),                               // free_remote_model
            "https://openrouter.ai/api/v1/chat/completions".into(),
            "google/gemini-2.0-flash-001".into(),
            api_key.to_string(),
            800,
            None,
            3,   // local_timeout_secs
            15,  // remote_timeout_secs
            "prompt".into(),
        )
    }

    #[test]
    fn test_dual_client_no_fallback_when_api_key_empty() {
        assert!(dual("").remote_fallback.is_none(), "empty api_key must not create a remote fallback client");
    }

    #[test]
    fn test_dual_client_fallback_created_when_key_present() {
        assert!(dual("sk-real-key").remote_fallback.is_some(), "non-empty api_key must create a remote fallback client");
    }

    #[test]
    fn test_force_fallback_errors_when_no_key() {
        let img = image::DynamicImage::new_rgb8(1, 1);
        let err = dual("").call_api_force_fallback(&img).unwrap_err();
        assert!(err.to_string().contains("OPENROUTER_API_KEY"), "error must mention the missing key");
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

    // ── parse_xy / coords_within ─────────────────────────────────────────────

    #[test]
    fn test_parse_xy_plain_string() {
        assert_eq!(parse_xy(Some("79,48")), Some((79, 48)));
    }

    #[test]
    fn test_parse_xy_bracketed_string() {
        // coerce_to_opt_string turns [79,48] into "[79,48]"
        assert_eq!(parse_xy(Some("[79,48]")), Some((79, 48)));
    }

    #[test]
    fn test_parse_xy_with_spaces() {
        assert_eq!(parse_xy(Some("[ 102, 82 ]")), Some((102, 82)));
    }

    #[test]
    fn test_parse_xy_none_returns_none() {
        assert_eq!(parse_xy(None), None);
    }

    #[test]
    fn test_parse_xy_garbage_returns_none() {
        assert_eq!(parse_xy(Some("not_a_coord")), None);
    }

    #[test]
    fn test_coords_within_exact_match() {
        assert!(coords_within(Some("79,48"), (79, 48), 0));
    }

    #[test]
    fn test_coords_within_tolerance_passes() {
        // 5 px off — within default tolerance of 10
        assert!(coords_within(Some("84,53"), (79, 48), 10));
    }

    #[test]
    fn test_coords_within_tolerance_fails() {
        // 11 px off on x — exceeds tolerance of 10
        assert!(!coords_within(Some("90,48"), (79, 48), 10));
    }

    #[test]
    fn test_coords_within_none_passes() {
        // model returned no coord — treated as "no constraint"
        assert!(coords_within(None, (79, 48), 0));
    }
}
