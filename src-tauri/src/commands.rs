//! Alle `#[tauri::command]`-Funktionen — die Brücke vom Frontend zum Rust-Backend.

use crate::backend::{refresh_ctx_max, register_ptt_shortcut, warmup_llm_async};
use crate::config::core_system_prompt;
use crate::pipeline::{process_pipeline, process_text_input};
use crate::state::ProcessingState;
use crate::window::reveal_main_window;
use crate::{voice, AppState, ChatMessage, VoiceConfig};
use tokio_util::sync::CancellationToken;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

#[tauri::command]
pub fn get_config(state: tauri::State<AppState>) -> VoiceConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn get_core_system_prompt() -> String {
    core_system_prompt().to_string()
}

#[tauri::command]
pub fn set_config(app: tauri::AppHandle, state: tauri::State<AppState>, config: VoiceConfig) {
    config.save();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_decorations(config.window.decorations);
    }

    let (old_hotkey, old_base, old_model, old_provider, old_prompt) = {
        let cfg = state.config.lock().unwrap();
        (
            cfg.hotkey.clone(),
            cfg.llm_base_url.clone(),
            cfg.llm_model.clone(),
            cfg.llm_provider.clone(),
            cfg.system_prompt.clone(),
        )
    };
    if old_hotkey != config.hotkey {
        let _ = app.global_shortcut().unregister(old_hotkey.as_str());
        if let Err(e) = register_ptt_shortcut(&app, &config.hotkey) {
            eprintln!("Failed to register hotkey {:?}: {}", config.hotkey, e);
            // Fall back to old hotkey so the user isn't left without PTT.
            let _ = register_ptt_shortcut(&app, &old_hotkey);
        }
    }

    let llm_endpoint_changed = old_base != config.llm_base_url
        || old_model != config.llm_model
        || old_provider != config.llm_provider;
    let warmup_needed = llm_endpoint_changed || old_prompt != config.system_prompt;

    *state.config.lock().unwrap() = config;
    let _ = app.emit("config_changed", ());

    if llm_endpoint_changed {
        refresh_ctx_max(&app);
    }
    if warmup_needed {
        warmup_llm_async(&app);
    }
}

#[tauri::command]
pub fn get_messages(state: tauri::State<AppState>) -> Vec<ChatMessage> {
    state.messages.lock().unwrap().clone()
}

#[tauri::command]
pub fn clear_messages(state: tauri::State<AppState>) {
    state.messages.lock().unwrap().clear();
}

#[tauri::command]
pub fn show_window(app: tauri::AppHandle) {
    reveal_main_window(&app);
}

#[tauri::command]
pub fn hide_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn get_ctx_max(state: tauri::State<AppState>) -> Option<u32> {
    *state.ctx_max.lock().unwrap()
}

#[tauri::command]
pub fn get_last_stats(state: tauri::State<AppState>) -> Option<voice::LlmStats> {
    state.last_stats.lock().unwrap().clone()
}

#[tauri::command]
pub async fn list_models(base_url: String, provider: String) -> Result<Vec<String>, String> {
    let base = base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .map_err(|e| e.to_string())?;

    if provider == "ollama" {
        // Ollama: /api/tags → { models: [{ name, ... }] }
        let resp = client
            .get(format!("{}/api/tags", base))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let mut out: Vec<String> = v
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out.dedup();
        return Ok(out);
    }

    // OpenAI-compatible (llama.cpp, vLLM, sglang, …): /v1/models → { data: [{ id }] }
    let resp = client
        .get(format!("{}/v1/models", base))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out: Vec<String> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|s| s.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    Ok(out)
}

// ── RAG Commands ──

#[tauri::command]
pub async fn ingest_text(
    app: tauri::AppHandle,
    source: String,
    text: String,
) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    state
        .rag_store
        .ingest(&source, &text, &config.llm_base_url, &config.embed_model)
        .await
        .map_err(|e| format!("Ingest failed: {}", e))
}

#[tauri::command]
pub async fn ingest_file(app: tauri::AppHandle, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Read failed: {}", e))?;
    let source = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    state
        .rag_store
        .ingest(&source, &text, &config.llm_base_url, &config.embed_model)
        .await
        .map_err(|e| format!("Ingest failed: {}", e))
}

#[tauri::command]
pub fn list_knowledge_sources(app: tauri::AppHandle) -> Result<Vec<(String, usize)>, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .list_sources()
        .map_err(|e| format!("List failed: {}", e))
}

#[tauri::command]
pub fn delete_knowledge_source(app: tauri::AppHandle, source: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .delete_source(&source)
        .map_err(|e| format!("Delete failed: {}", e))
}

#[tauri::command]
pub fn start_recording(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    {
        let is_rec = state.is_recording.lock().unwrap();
        if *is_rec {
            return Ok(());
        }
    }

    state.recorded_samples.lock().unwrap().clear();
    *state.is_recording.lock().unwrap() = true;

    let app_handle = app.clone();
    // Spawn recording on a dedicated thread (cpal::Stream isn't Send).
    std::thread::spawn(move || {
        if let Err(e) = voice::record_audio(&app_handle) {
            eprintln!("Recording error: {}", e);
        }
    });

    Ok(())
}

/// Wird vom Lautsprecher-Toggle (Orb-Icon und Tray-Menü) aufgerufen.
/// Schreibt nur das eine Feld, ohne den Rest der Config über den Draht zu schicken.
#[tauri::command]
pub fn set_tts_enabled(app: tauri::AppHandle, state: tauri::State<AppState>, enabled: bool) {
    {
        let mut cfg = state.config.lock().unwrap();
        if cfg.tts_enabled == enabled {
            return;
        }
        cfg.tts_enabled = enabled;
        cfg.save();
    }
    let _ = app.emit("config_changed", ());
}

/// Schickt einen vom User getippten Text in die LLM-Pipeline, als wäre er
/// per STT angekommen. Bestehende Pipelines werden vorher abgebrochen, damit
/// nicht zwei parallele LLM-Calls in dieselbe Bubble streamen.
#[tauri::command]
pub fn submit_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("No text provided".to_string());
    }

    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    let cancel_token = {
        let mut cancel = state.pipeline_cancel.lock().unwrap();
        cancel.cancel();
        *cancel = CancellationToken::new();
        cancel.clone()
    };
    let _ = app.emit("pipeline_interrupted", ());

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            process_text_input(app_handle.clone(), text, config, cancel_token).await
        {
            if e != "interrupted" {
                eprintln!("Text pipeline error: {}", e);
                let _ = app_handle.emit(
                    "processing",
                    ProcessingState {
                        stage: "error".to_string(),
                        text: e,
                    },
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn stop_recording_and_process(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    *state.is_recording.lock().unwrap() = false;

    // Give a moment for the recording thread to finish writing samples.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let samples = state.recorded_samples.lock().unwrap().clone();
    let sample_rate = *state.recording_sample_rate.lock().unwrap();
    let config = state.config.lock().unwrap().clone();

    if samples.is_empty() {
        return Err("No audio recorded".to_string());
    }

    let cancel_token = state.pipeline_cancel.lock().unwrap().clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = process_pipeline(
            app_handle.clone(),
            samples,
            sample_rate,
            config,
            cancel_token,
        )
        .await
        {
            if e != "interrupted" {
                eprintln!("Pipeline error: {}", e);
                let _ = app_handle.emit(
                    "processing",
                    ProcessingState {
                        stage: "error".to_string(),
                        text: e,
                    },
                );
            }
        }
    });

    Ok(())
}
