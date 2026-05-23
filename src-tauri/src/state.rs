//! Geteilter In-Memory-Zustand und Event-Typen für die UI.

use crate::voice;
use crate::{rag, sandbox, VoiceConfig};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
pub struct ProcessingState {
    pub stage: String,
    pub text: String,
}

impl Default for ProcessingState {
    fn default() -> Self {
        Self {
            stage: "idle".to_string(),
            text: String::new(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct AudioChunk {
    pub index: u32,
    pub audio: String, // base64 WAV
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Preserved tool_calls from assistant messages.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<voice::OllamaToolCallOut>>,
    /// OpenAI-compatible tool result messages must reference the call they answer.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct PanelInfo {
    pub title: String,
    pub content: String,
}

#[derive(Default)]
pub struct UiState {
    pub panel: Option<PanelInfo>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DialogOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DialogPayload {
    pub question: String,
    pub options: Vec<DialogOption>,
}

pub struct DialogState {
    pub question: String,
    pub options: Vec<DialogOption>,
    pub responder: oneshot::Sender<String>,
}

#[derive(Clone, Serialize)]
pub struct AutomationEvent {
    pub ts: String,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConsoleError {
    pub ts: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stack: Option<String>,
}

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppMode {
    #[default]
    Chat,
    CodexSession,
    ClaudeSession,
    OpencodeSession,
    AgySession,
}

impl std::fmt::Display for AppMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Chat => write!(f, "chat"),
            Self::CodexSession => write!(f, "codex_session"),
            Self::ClaudeSession => write!(f, "claude_session"),
            Self::OpencodeSession => write!(f, "opencode_session"),
            Self::AgySession => write!(f, "agy_session"),
        }
    }
}

pub struct AppState {
    pub app_mode: Mutex<AppMode>,
    pub messages: Mutex<Vec<ChatMessage>>,
    pub config: Mutex<VoiceConfig>,
    pub ui_state: Mutex<UiState>,
    pub pending_dialog: Mutex<Option<DialogState>>,
    pub processing: Mutex<ProcessingState>,
    pub automation_events: Mutex<Vec<AutomationEvent>>,
    pub console_errors: Mutex<Vec<ConsoleError>>,
    pub rag_store: rag::RagStore,
    pub audit_log: Mutex<sandbox::AuditLog>,
    /// Audio samples collected by the recording thread.
    pub recorded_samples: Mutex<Vec<f32>>,
    pub recording_sample_rate: Mutex<u32>,
    pub is_recording: Mutex<bool>,
    /// Cancellation token for the active pipeline — cancelled when user interrupts.
    pub pipeline_cancel: Mutex<CancellationToken>,
    /// Discovered max context window for the configured LLM model, if known.
    pub ctx_max: Mutex<Option<u32>>,
    /// Last emitted LLM stats, so the frontend can recover them on (re-)mount
    /// even if the event fired before it subscribed (e.g. warmup during setup).
    pub last_stats: Mutex<Option<voice::LlmStats>>,
}

pub fn update_processing_state(app: &tauri::AppHandle, processing: ProcessingState) {
    let state = app.state::<AppState>();
    *state.processing.lock().unwrap() = processing;
}

pub fn emit_processing(
    app: &tauri::AppHandle,
    processing: ProcessingState,
) -> Result<(), tauri::Error> {
    update_processing_state(app, processing.clone());
    app.emit("processing", processing)
}

pub fn record_automation_event(app: &tauri::AppHandle, kind: &str, message: impl Into<String>) {
    let state = app.state::<AppState>();
    let mut events = state.automation_events.lock().unwrap();
    events.push(AutomationEvent {
        ts: Utc::now().to_rfc3339(),
        kind: kind.to_string(),
        message: message.into(),
    });
    trim_oldest(&mut events, 200);
}

pub fn record_console_error(
    app: &tauri::AppHandle,
    message: impl Into<String>,
    source: Option<String>,
    stack: Option<String>,
) {
    let state = app.state::<AppState>();
    let mut errors = state.console_errors.lock().unwrap();
    errors.push(ConsoleError {
        ts: Utc::now().to_rfc3339(),
        message: message.into(),
        source,
        stack,
    });
    trim_oldest(&mut errors, 100);
}

fn trim_oldest<T>(items: &mut Vec<T>, max_len: usize) {
    if items.len() > max_len {
        let overflow = items.len() - max_len;
        items.drain(0..overflow);
    }
}
