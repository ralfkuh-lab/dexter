//! User-Konfiguration mit JSON-Persistenz in `~/.config/voice-assistant/config.json`.
//! Wenn die Datei nicht existiert, greifen die Defaults — sobald der User
//! aber speichert, sind seine Werte eingefroren (Default-Updates greifen nicht
//! mehr für ihn). Bei Feld-Umbenennungen `serde(alias = …)` verwenden.

use crate::sandbox;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub search_knowledge: bool,
    #[serde(default = "default_true")]
    pub screenshot: bool,
    #[serde(default = "default_true")]
    pub read_clipboard: bool,
    #[serde(default = "default_true")]
    pub open_url: bool,
    #[serde(default = "default_true")]
    pub get_current_time: bool,
    #[serde(default = "default_true")]
    pub list_apps: bool,
    #[serde(default)]
    pub run_command: bool,
    #[serde(default = "default_true")]
    pub web_fetch: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            search_knowledge: false,
            screenshot: false,
            read_clipboard: false,
            open_url: false,
            get_current_time: false,
            list_apps: false,
            run_command: false, // Off by default — powerful tool
            web_fetch: false,
        }
    }
}

fn default_llm_provider() -> String {
    "openai".to_string()
}

fn default_llm_base_url() -> String {
    "http://127.0.0.1:8081".to_string()
}

fn default_llm_model() -> String {
    "gemma".to_string()
}

fn default_whisper_server_url() -> String {
    "http://127.0.0.1:8350".to_string()
}

fn default_tts_url() -> String {
    "http://127.0.0.1:8005".to_string()
}

fn default_tts_voice() -> String {
    "de_DE-thorsten-medium".to_string()
}

pub fn core_system_prompt() -> &'static str {
    "You are a desktop voice assistant. Answer in the user's preferred language. Keep answers brief. Use available tools for current, local, or computer-specific information. If the user asks for date or time, use get_current_time. Do not claim you checked something unless you called the matching tool."
}

fn default_user_prompt() -> String {
    "Sprich grundsätzlich Deutsch, außer der Nutzer bittet ausdrücklich um eine andere Sprache. Halte Antworten kurz und gesprächig. Nutze keine Markdown-Formatierung, keine Codeblöcke und keine Aufzählungen.".to_string()
}

fn default_window_width() -> f64 {
    380.0
}
fn default_window_height() -> f64 {
    420.0
}

fn default_hotkey() -> String {
    "F9".to_string()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default)]
    pub decorations: bool,
    #[serde(default = "default_window_width")]
    pub width: f64,
    #[serde(default = "default_window_height")]
    pub height: f64,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            decorations: false,
            width: default_window_width(),
            height: default_window_height(),
            x: None,
            y: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default = "default_whisper_server_url")]
    pub whisper_server_url: String,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    #[serde(default = "default_llm_base_url", alias = "ollama_url")]
    pub llm_base_url: String,
    #[serde(default = "default_llm_model", alias = "ollama_model")]
    pub llm_model: String,
    #[serde(default)]
    pub embed_model: String,
    #[serde(default)]
    pub vision_model: String,
    #[serde(default = "default_tts_url", alias = "chatterbox_url")]
    pub tts_url: String,
    #[serde(default = "default_tts_voice", alias = "chatterbox_voice")]
    pub tts_voice: String,
    #[serde(default)]
    pub debug_bubbles: bool,
    #[serde(default = "default_user_prompt")]
    pub system_prompt: String,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: sandbox::SandboxConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_true")]
    pub show_stats: bool,
    #[serde(default = "default_true")]
    pub tts_enabled: bool,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            whisper_server_url: default_whisper_server_url(),
            llm_provider: default_llm_provider(),
            llm_base_url: default_llm_base_url(),
            llm_model: default_llm_model(),
            embed_model: String::new(),
            vision_model: String::new(),
            tts_url: default_tts_url(),
            tts_voice: default_tts_voice(),
            debug_bubbles: false,
            system_prompt: default_user_prompt(),
            tools: ToolsConfig::default(),
            sandbox: sandbox::SandboxConfig::default(),
            window: WindowConfig::default(),
            hotkey: default_hotkey(),
            show_stats: true,
            tts_enabled: true,
        }
    }
}

impl VoiceConfig {
    pub fn effective_system_prompt(&self) -> String {
        let user_prompt = self.system_prompt.trim();
        if user_prompt.is_empty() {
            core_system_prompt().to_string()
        } else {
            format!(
                "{}\n\nEditable user prompt:\n{}",
                core_system_prompt(),
                user_prompt
            )
        }
    }

    fn config_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("voice-assistant")
            .join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str(&data) {
                    return config;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, data);
        }
    }
}
