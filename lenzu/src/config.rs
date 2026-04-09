use isolang::Language;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use serde_json;

/// Controls which field(s) from each TranslationResult are sent to the overlay HUD.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OverlayRenderMode {
    Original,
    English,
    Furigana,
    Romaji,
    /// All text fields (original, english, furigana, romaji) joined per result.
    All,
    /// Same as All plus bounding-box coordinates and debug_info.
    Debug,
}

// Full-image prompt (Phase 1 / DBNet fallback when no regions are found).
// {src}/{dest} are substituted at runtime by resolved_prompt().
const TRANSLATE_PROMPT: &str =
    "Act as a highly accurate {src}-to-{dest} OCR and translation engine. \
    Extract ALL text from the image. Return a JSON array of objects, one per line/bubble found. \
    Each object MUST have these fields with EXACTLY these types: \
    'original' (string), \
    'top_xy' (string in \"x,y\" format, e.g. \"79,48\" — upper-left pixel corner of the text bounding box), \
    'bot_xy' (string in \"x,y\" format, e.g. \"167,181\" — lower-right pixel corner of the text bounding box), \
    'debug_info' (string or null). \
    Do NOT use arrays or objects for top_xy/bot_xy — they must be plain strings.";

// Per-region prompt used when DBNet detects text regions (Phase 4).
// The LLM receives exactly one crop; no coordinate fields are needed.
// {src}/{dest} and {extra_prompt} are substituted at runtime.
const TRANSLATE_PROMPT_PER_REGION: &str =
    "Act as a highly accurate {src}-to-{dest} OCR and translation engine. \
    The image contains exactly one text region. Extract the text and translate it. \
    Return a JSON object with these fields: \
    'original' (string — the exact text as written), \
    'debug_info' (string or null). \
    {extra_prompt}";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub lens_size: i32,
    pub ui_panel_height: i32,
    pub font_size: f64,
    pub hud_color_hex: String,
    pub show_romaji: bool,
    pub show_furigana: bool,
    pub overlay_enabled: bool,
    pub overlay_udp_port: u16,
    /// Primary backend — ollama (local). No API key required.
    pub llm_api_endpoint: String,
    pub llm_default_model: String,
    /// Free-tier remote backend — OpenRouter free model.
    /// Available without an API key (30 req/day unauthenticated; 1000/day with key).
    /// Tried before the paid `fallback_llm_*` backend in the fallback chain.
    #[serde(default = "default_free_remote_endpoint")]
    pub free_remote_endpoint: String,
    #[serde(default = "default_free_remote_model")]
    pub free_remote_model: String,
    /// Paid fallback backend — OpenRouter (remote). Requires OPENROUTER_API_KEY.
    #[serde(default = "default_fallback_endpoint")]
    pub fallback_llm_api_endpoint: String,
    #[serde(default = "default_fallback_model")]
    pub fallback_llm_model: String,
    /// Longest-edge pixel limit applied to images before the fallback call. 0 = no limit.
    #[serde(default = "default_fallback_max_dimension")]
    pub fallback_max_dimension: u32,
    /// Longest-edge pixel limit applied to images before every primary (local Ollama) call.
    /// `0` = no limit (default — preserves existing behaviour).
    /// Set to e.g. `512` to reduce payload size and VRAM pressure on smaller local models.
    /// Remote fallbacks are unaffected — they use `fallback_max_dimension`.
    #[serde(default)]
    pub primary_max_dimension: u32,
    /// Timeout in seconds for each local (ollama) backend request.
    /// If inference doesn't complete within this window the client fails-over to the
    /// next backend in the chain.  Default: 3 s — fast enough to feel responsive while
    /// still giving GPU inference a chance to finish on typical hardware.
    #[serde(default = "default_local_timeout_secs")]
    pub local_timeout_secs: u64,
    /// Timeout in seconds for the free remote (openrouter/free) tier.  Default: 15 s.
    /// If the free tier doesn't respond in this window the paid remote is tried next.
    #[serde(default = "default_remote_timeout_secs")]
    pub remote_timeout_secs: u64,
    /// Timeout in seconds for the paid remote (OpenRouter/Gemini) backend.  Default: 60 s.
    /// Higher because paid inference is worth waiting longer for.
    #[serde(default = "default_paid_remote_timeout_secs")]
    pub paid_remote_timeout_secs: u64,
    /// Ollama KV-cache context size for the primary (local) backend.
    /// Smaller values (e.g. 2048) free VRAM on cards with < 1 GB headroom after model load.
    /// `None` = use ollama's default (usually 4096).
    #[serde(default)]
    pub primary_num_ctx: Option<u32>,
    /// Ordered list of local fallback models tried after the primary.  Each uses the same
    /// ollama endpoint as the primary.  Tried in order; remote fallback is attempted last.
    /// Example: `["glm-ocr", "qwen2.5vl:7b"]`.  Empty = skip straight to remote.
    #[serde(default)]
    pub local_fallback_models: Vec<String>,
    /// How long (in seconds) to keep the lens visible after a capture result arrives.
    /// Set to 0 to hide immediately once Shift is released.
    #[serde(default = "default_result_display_secs")]
    pub result_display_secs: u64,
    // ── Phase 4: text detection (DBNet) ───────────────────────────────────────
    /// Path to the DBNet ONNX model file (relative to CWD).
    /// When absent or null, text detection is disabled and full-image OCR is used.
    /// Defaults to `"assets/stabrise-text_detection_dbnet_ml_v02_model.onnx"` so
    /// DBNet works out-of-the-box without any config entry.  Set to `null` to disable.
    #[serde(default = "default_text_detection_model")]
    pub text_detection_model: Option<String>,
    /// DBNet probability threshold.  Default: 0.2.
    /// Lower values detect more text at the cost of more false positives.
    #[serde(default = "default_text_detection_threshold")]
    pub text_detection_threshold: f32,
    /// Morphological dilation radius applied to the binary mask at 640×640.  Default: 16.
    /// Merges nearby character blobs; compensates for DBNet's slightly-shrunk training targets.
    #[serde(default = "default_text_detection_dilation")]
    pub text_detection_dilation: u8,
    /// Horizontal padding (original-image pixels) added to each bounding box.  Default: 32.
    #[serde(default = "default_text_detection_pad_x")]
    pub text_detection_pad_x: u32,
    /// Vertical padding (original-image pixels) added to each bounding box.  Default: 32.
    #[serde(default = "default_text_detection_pad_y")]
    pub text_detection_pad_y: u32,
    /// Oversample multiplier: actual capture size = max(lens_size × factor, 640).  Default: 2.0.
    #[serde(default = "default_text_detection_oversample_factor")]
    pub text_detection_oversample_factor: f32,
    /// Hard ceiling on the oversample capture dimension (pixels).  Default: 1600.
    #[serde(default = "default_text_detection_max_capture_size")]
    pub text_detection_max_capture_size: u32,
    /// Padding added around the final union-crop on all sides (pixels).  Default: 16.
    #[serde(default = "default_text_detection_crop_padding")]
    pub text_detection_crop_padding: u32,
    pub translate_src: Language,
    pub translate_dest: Language,
    pub translate_extra_prompt: String,
    pub overlay_render_mode: OverlayRenderMode,
}

fn default_free_remote_endpoint() -> String {
    "https://openrouter.ai/api/v1/chat/completions".to_string()
}
fn default_free_remote_model() -> String {
    "openrouter/free".to_string()
}
fn default_fallback_endpoint() -> String {
    "https://openrouter.ai/api/v1/chat/completions".to_string()
}
fn default_fallback_model() -> String {
    "google/gemini-2.0-flash-001".to_string()
}
fn default_fallback_max_dimension() -> u32 {
    800
}
fn default_local_timeout_secs() -> u64 {
    3
}
fn default_remote_timeout_secs() -> u64 {
    15
}
fn default_paid_remote_timeout_secs() -> u64 {
    60
}
fn default_result_display_secs() -> u64 {
    5
}
fn default_text_detection_model() -> Option<String> {
    Some("assets/stabrise-text_detection_dbnet_ml_v02_model.onnx".to_string())
}
fn default_text_detection_threshold() -> f32 {
    0.2
}
fn default_text_detection_dilation() -> u8 {
    16
}
fn default_text_detection_pad_x() -> u32 {
    32
}
fn default_text_detection_pad_y() -> u32 {
    32
}
fn default_text_detection_oversample_factor() -> f32 {
    2.0
}
fn default_text_detection_max_capture_size() -> u32 {
    1600
}
fn default_text_detection_crop_padding() -> u32 {
    16
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            lens_size: 400,
            ui_panel_height: 130,
            font_size: 13.0,
            hud_color_hex: "#00FFCC".to_string(), // Cyberpunk teal
            show_romaji: true,
            show_furigana: true,
            overlay_enabled: true,
            overlay_udp_port: 7331,
            // Primary: glm-ocr — fast OCR specialist (~15s), no API key needed
            llm_api_endpoint: "http://localhost:11434/v1/chat/completions".to_string(),
            llm_default_model: "glm-ocr".to_string(),
            // Free remote: OpenRouter free tier — no API key needed (30 req/day anon; 1000/day with key).
            free_remote_endpoint: default_free_remote_endpoint(),
            free_remote_model: default_free_remote_model(),
            // Paid fallback: OpenRouter (remote) — requires OPENROUTER_API_KEY.
            // or when Ctrl+Shift+Click forces remote.
            fallback_llm_api_endpoint: default_fallback_endpoint(),
            fallback_llm_model: default_fallback_model(),
            fallback_max_dimension: default_fallback_max_dimension(),
            primary_max_dimension: 0,
            local_timeout_secs: default_local_timeout_secs(),
            remote_timeout_secs: default_remote_timeout_secs(),
            paid_remote_timeout_secs: default_paid_remote_timeout_secs(),
            primary_num_ctx: None,
            // gemma4:e2b as local fallback — slow (~50s) but works offline with no API key
            local_fallback_models: vec!["gemma4:e2b".to_string()],
            result_display_secs: default_result_display_secs(),
            text_detection_model: default_text_detection_model(),
            text_detection_threshold: default_text_detection_threshold(),
            text_detection_dilation: default_text_detection_dilation(),
            text_detection_pad_x: default_text_detection_pad_x(),
            text_detection_pad_y: default_text_detection_pad_y(),
            text_detection_oversample_factor: default_text_detection_oversample_factor(),
            text_detection_max_capture_size: default_text_detection_max_capture_size(),
            text_detection_crop_padding: default_text_detection_crop_padding(),
            translate_src: Language::Jpn,
            translate_dest: Language::Eng,
            // Japanese-specific: add furigana and romaji fields with reading format hint
            translate_extra_prompt:
                "Each object must also include: furigana (format: 漢字[かんじ]) and romaji fields, plus english translation."
                    .to_string(),
            overlay_render_mode: OverlayRenderMode::Furigana,
        }
    }
}

impl AppConfig {
    /// Full-image prompt (Phase 1 / fallback when DBNet finds no regions).
    /// Substitutes {src}/{dest} and appends translate_extra_prompt.
    pub fn resolved_prompt(&self) -> String {
        let base = TRANSLATE_PROMPT
            .replace("{src}", self.translate_src.to_name())
            .replace("{dest}", self.translate_dest.to_name());
        if self.translate_extra_prompt.is_empty() {
            base
        } else {
            format!("{} {}", base, self.translate_extra_prompt)
        }
    }

    /// Per-region prompt (Phase 4): used when DBNet finds text regions.
    /// The LLM receives one crop and does not need to report coordinates.
    pub fn resolved_per_region_prompt(&self) -> String {
        TRANSLATE_PROMPT_PER_REGION
            .replace("{src}", self.translate_src.to_name())
            .replace("{dest}", self.translate_dest.to_name())
            .replace("{extra_prompt}", &self.translate_extra_prompt)
    }

    pub fn load() -> Self {
        let path = Path::new("lenzu_config.json");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(config) = serde_json::from_str(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_old_config_without_fallback_fields_loads_with_defaults() {
        // Simulate a lenzu_config.json that was saved before the fallback fields existed.
        // serde(default) must fill them in without returning an error.
        let json = r##"{
            "lens_size": 400,
            "ui_panel_height": 130,
            "font_size": 13.0,
            "hud_color_hex": "#00FFCC",
            "show_romaji": true,
            "show_furigana": true,
            "overlay_enabled": true,
            "overlay_udp_port": 7331,
            "llm_api_endpoint": "http://localhost:11434/v1/chat/completions",
            "llm_default_model": "gemma4:e2b",
            "translate_src": "jpn",
            "translate_dest": "eng",
            "translate_extra_prompt": "",
            "overlay_render_mode": "furigana"
        }"##;
        let cfg: AppConfig = serde_json::from_str(json).expect("old config must deserialize");
        assert_eq!(cfg.fallback_llm_api_endpoint, "https://openrouter.ai/api/v1/chat/completions");
        assert_eq!(cfg.fallback_llm_model, "google/gemini-2.0-flash-001");
        assert_eq!(cfg.fallback_max_dimension, 800);
        assert_eq!(cfg.primary_max_dimension, 0, "old configs without primary_max_dimension must default to 0 (no limit)");
    }

    #[test]
    fn test_config_defaults() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.lens_size, 400);
        assert!(cfg.hud_color_hex.starts_with('#'));
    }

    #[test]
    fn test_prompt_specifies_xy_string_format() {
        // If these fail, the prompt no longer tells the LLM the required format
        // and models will start returning [x,y] arrays instead of "x,y" strings.
        let cfg = AppConfig::default();
        let prompt = cfg.resolved_prompt();
        assert!(
            prompt.contains("\"x,y\""),
            "prompt must specify x,y string format for bounding box fields"
        );
        assert!(
            prompt.contains("top_xy") && prompt.contains("bot_xy"),
            "prompt must name both bounding box fields"
        );
        assert!(
            prompt.contains("Do NOT use arrays"),
            "prompt must explicitly forbid array format for top_xy/bot_xy"
        );
    }

    #[test]
    fn test_prompt_resolves_src_dest() {
        let cfg = AppConfig::default();
        let prompt = cfg.resolved_prompt();
        assert!(!prompt.contains("{src}"), "{{src}} placeholder must be resolved");
        assert!(!prompt.contains("{dest}"), "{{dest}} placeholder must be resolved");
        assert!(prompt.contains("Japanese"), "default src language should appear");
        assert!(prompt.contains("English"), "default dest language should appear");
    }
}
