//! Geteilter In-Memory-Zustand und Event-Typen für die UI.

use crate::voice;
use crate::{rag, sandbox, VoiceConfig};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
pub struct ProcessingState {
    pub stage: String,
    pub text: String,
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

pub struct AppState {
    pub messages: Mutex<Vec<ChatMessage>>,
    pub config: Mutex<VoiceConfig>,
    pub ui_state: Mutex<UiState>,
    pub pending_dialog: Mutex<Option<DialogState>>,
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
