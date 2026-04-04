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

// Base prompt — language-agnostic. {src}/{dest} are substituted at runtime.
const TRANSLATE_PROMPT: &str =
    "Act as a highly accurate {src}-to-{dest} OCR and translation engine. \
    Extract ALL text from the image. Return a JSON array of objects, one per line/bubble found. \
    Each object MUST have these fields with EXACTLY these types: \
    'original' (string), \
    'top_xy' (string in \"x,y\" format, e.g. \"79,48\" — upper-left pixel corner of the text bounding box), \
    'bot_xy' (string in \"x,y\" format, e.g. \"167,181\" — lower-right pixel corner of the text bounding box), \
    'debug_info' (string or null). \
    Do NOT use arrays or objects for top_xy/bot_xy — they must be plain strings.";

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
    pub llm_api_endpoint: String,
    pub llm_default_model: String,
    pub translate_src: Language,
    pub translate_dest: Language,
    pub translate_extra_prompt: String,
    pub overlay_render_mode: OverlayRenderMode,
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
            // we're defaulting with Openrouter; I've no intention at this time to support other AI
            // endpoints as my paid service; you'll have to do your own juggling if you want
            // google, openai, etc as your target (good luck!)
            llm_api_endpoint: "https://openrouter.ai/api/v1/chat/completions".to_string(),
            llm_default_model: "google/gemini-2.0-flash-001".to_string(), // I've spent about USD: $0.0006 (less than a penny) per hour
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
    /// Returns the full prompt: base with {src}/{dest} resolved, plus any extra appended.
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
