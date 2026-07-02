//! HTTP-Clients für die externe Sprach-Pipeline (STT → LLM → TTS) plus die
//! lokale Mikrofon-Aufnahme. Submodule sind nach Pipeline-Schritt getrennt.

use crate::AppState;
use serde::Serialize;
use tauri::{Emitter, Manager};

mod audio;
mod llm;
mod stt;
mod tool_defs;
mod tts;

pub use audio::{record_audio, record_continuous, AudioSegment};
pub use llm::{
    chat_streaming, warmup_llm, StreamResult, ToolCall, ToolCallOut, ToolCallSource,
};
pub use stt::transcribe_audio_http;
pub use tool_defs::build_tools;
pub use tts::synthesize;

#[derive(Serialize, Clone, Debug, Default)]
pub struct LlmStats {
    pub ttft_ms: Option<u64>,
    pub tokens_per_sec: Option<f64>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub ctx_max: Option<u32>,
    /// Model id reported by the endpoint in the streaming response (may differ
    /// from what the user picked in settings, e.g. when the server only serves
    /// one model regardless of the request body).
    pub model: Option<String>,
}

pub(crate) fn emit_llm_stats(app: &tauri::AppHandle, mut stats: LlmStats) {
    {
        let state = app.state::<AppState>();
        if stats.ctx_max.is_none() {
            stats.ctx_max = *state.ctx_max.lock().unwrap();
        }
        *state.last_stats.lock().unwrap() = Some(stats.clone());
    }
    let _ = app.emit("llm_stats", stats);
}

pub(crate) fn trim_base_url(url: &str) -> &str {
    url.trim_end_matches('/')
}
