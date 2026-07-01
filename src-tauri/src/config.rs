//! User-Konfiguration mit JSON-Persistenz in `~/.config/voice-assistant/config.json`.
//! Wenn die Datei nicht existiert, greifen die Defaults — sobald der User
//! aber speichert, sind seine Werte eingefroren (Default-Updates greifen nicht
//! mehr für ihn). Bei Feld-Umbenennungen `serde(alias = …)` verwenden.

use crate::sandbox;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true", alias = "search_knowledge")]
    pub search_notes: bool,
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
    #[serde(default = "default_true")]
    pub show_panel: bool,
    #[serde(default = "default_true")]
    pub ask_user: bool,
    #[serde(default = "default_true")]
    pub switch_mode: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            search_notes: false,
            screenshot: false,
            read_clipboard: false,
            open_url: false,
            get_current_time: false,
            list_apps: false,
            run_command: false, // Off by default — powerful tool
            web_fetch: false,
            show_panel: true,
            ask_user: true,
            switch_mode: true,
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

const FALLBACK_SYSTEM_PROMPT: &str = "You are Dexter, a desktop voice assistant. Your responses are spoken aloud via TTS. Keep answers short. Use tools when needed.";

pub fn core_system_prompt() -> &'static str {
    static PROMPT: OnceLock<String> = OnceLock::new();
    PROMPT.get_or_init(|| {
        let prompt_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default()
            .join("../../system-prompt.md");

        // Auch direkt im Projekt-Root suchen (für dev-Modus)
        let candidates = [
            prompt_path,
            std::path::PathBuf::from("system-prompt.md"),
            std::env::current_dir()
                .unwrap_or_default()
                .join("system-prompt.md"),
            std::env::current_dir()
                .unwrap_or_default()
                .join("../system-prompt.md"),
        ];

        for path in &candidates {
            if let Ok(content) = std::fs::read_to_string(path) {
                let trimmed = content
                    .strip_prefix("# Dexter System-Prompt\n")
                    .unwrap_or(&content);
                let trimmed = if let Some(pos) = trimmed.find("\n---\n") {
                    &trimmed[pos + 5..]
                } else {
                    trimmed
                };
                eprintln!("System prompt loaded from {}", path.display());
                return trimmed.trim().to_string();
            }
        }

        eprintln!("WARNING: system-prompt.md not found, using fallback");
        FALLBACK_SYSTEM_PROMPT.to_string()
    })
}

static SYSTEM_INFO_CACHE: OnceLock<String> = OnceLock::new();

pub fn system_info() -> &'static str {
    SYSTEM_INFO_CACHE.get_or_init(|| {
        let mut parts = Vec::new();

        // User + Home
        if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")) {
            parts.push(format!("User: {}", user));
        }
        if let Some(home) = dirs::home_dir() {
            parts.push(format!("Home: {}", home.display()));
        }

        // Hostname
        if let Ok(hostname) = std::fs::read_to_string("/etc/hostname") {
            parts.push(format!("Host: {}", hostname.trim()));
        }

        // OS
        if let Ok(os_release) = std::fs::read_to_string("/etc/os-release") {
            if let Some(pretty) = os_release
                .lines()
                .find(|l| l.starts_with("PRETTY_NAME="))
                .and_then(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"'))
            {
                parts.push(format!("OS: {}", pretty));
            }
        }

        // Kernel
        if let Ok(output) = std::process::Command::new("uname").arg("-r").output() {
            if output.status.success() {
                parts.push(format!(
                    "Kernel: {}",
                    String::from_utf8_lossy(&output.stdout).trim()
                ));
            }
        }

        // CPU
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            if let Some(model) = cpuinfo
                .lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim())
            {
                let cores = cpuinfo
                    .lines()
                    .filter(|l| l.starts_with("processor"))
                    .count();
                parts.push(format!("CPU: {} ({} threads)", model, cores));
            }
        }

        // RAM
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            if let Some(total_kb) = meminfo
                .lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| {
                    l.split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<u64>().ok())
                })
            {
                parts.push(format!("RAM: {} GB", total_kb / 1_048_576));
            }
        }

        // GPU (nvidia-smi)
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ])
            .output()
        {
            if output.status.success() {
                let line = String::from_utf8_lossy(&output.stdout);
                let line = line.trim();
                if !line.is_empty() {
                    if let Some((name, mem)) = line.split_once(',') {
                        parts.push(format!("GPU: {} ({} MiB VRAM)", name.trim(), mem.trim()));
                    }
                }
            }
        }

        // Shell
        if let Ok(shell) = std::env::var("SHELL") {
            parts.push(format!("Shell: {}", shell));
        }

        if parts.is_empty() {
            "System info unavailable.".to_string()
        } else {
            parts.join("\n")
        }
    })
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

fn default_dictation_hotkey() -> String {
    "F10".to_string()
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
    pub vault_path: String,
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
    #[serde(default = "default_dictation_hotkey")]
    pub dictation_hotkey: String,
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
            vault_path: String::new(),
            vision_model: String::new(),
            tts_url: default_tts_url(),
            tts_voice: default_tts_voice(),
            debug_bubbles: false,
            system_prompt: default_user_prompt(),
            tools: ToolsConfig::default(),
            sandbox: sandbox::SandboxConfig::default(),
            window: WindowConfig::default(),
            hotkey: default_hotkey(),
            dictation_hotkey: default_dictation_hotkey(),
            show_stats: true,
            tts_enabled: true,
        }
    }
}

impl VoiceConfig {
    pub fn effective_system_prompt(&self) -> String {
        let mut prompt = core_system_prompt().to_string();

        prompt.push_str("\n\n# System environment\n");
        prompt.push_str(system_info());

        let user_prompt = self.system_prompt.trim();
        if !user_prompt.is_empty() {
            prompt.push_str("\n\n# User instructions\n");
            prompt.push_str(user_prompt);
        }

        prompt
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
