use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub lens_size: i32,
    pub ui_panel_height: i32,
    pub font_size: f64,
    pub hud_color_hex: String,
    pub show_romaji: bool,
    pub show_furigana: bool,
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
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = Path::new("config.toml");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(config) = toml::from_str(&content) {
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
}
