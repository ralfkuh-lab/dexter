//! LLM-Backend-Discovery (Kontextfenster) und PTT-Shortcut-Registrierung.

use crate::pipeline;
use crate::AppState;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Best-effort: ask the LLM backend for the active model's max context window.
/// Tries llama.cpp `/props` first, then OpenAI-style `/v1/models` (vLLM exposes
/// `max_model_len`). Returns None if nothing matches.
async fn discover_ctx_max(base_url: &str, model: &str) -> Option<u32> {
    let base = base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    // 1) llama.cpp /props → default_generation_settings.n_ctx (or top-level n_ctx)
    if let Ok(resp) = client.get(format!("{}/props", base)).send().await {
        if let Ok(v) = resp.json::<serde_json::Value>().await {
            let candidates = [
                v.pointer("/default_generation_settings/n_ctx"),
                v.pointer("/default_generation_settings/n_ctx_train"),
                v.pointer("/n_ctx"),
            ];
            for c in candidates.iter().flatten() {
                if let Some(n) = c.as_u64() {
                    return Some(n as u32);
                }
            }
        }
    }

    // 2) OpenAI-style /v1/models — pick the entry whose id matches `model`,
    //    look for common context-window fields.
    if let Ok(resp) = client.get(format!("{}/v1/models", base)).send().await {
        if let Ok(v) = resp.json::<serde_json::Value>().await {
            if let Some(data) = v.get("data").and_then(|d| d.as_array()) {
                let entry = data
                    .iter()
                    .find(|e| e.get("id").and_then(|s| s.as_str()) == Some(model))
                    .or_else(|| data.first());
                if let Some(e) = entry {
                    for key in ["max_model_len", "context_window", "context_length", "n_ctx"] {
                        if let Some(n) = e.get(key).and_then(|x| x.as_u64()) {
                            return Some(n as u32);
                        }
                    }
                }
            }
        }
    }

    None
}

pub fn refresh_ctx_max(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (base, model) = {
            let state = app.state::<AppState>();
            let cfg = state.config.lock().unwrap();
            (cfg.llm_base_url.clone(), cfg.llm_model.clone())
        };
        let ctx = discover_ctx_max(&base, &model).await;
        {
            let state = app.state::<AppState>();
            *state.ctx_max.lock().unwrap() = ctx;
        }
        let _ = app.emit("config_changed", ());
    });
}

/// Prime the LLM prompt cache with the static prefix in the background.
pub fn warmup_llm_async(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let cfg = { app.state::<AppState>().config.lock().unwrap().clone() };
        crate::voice::warmup_llm(&app, &cfg).await;
    });
}

pub fn register_ptt_shortcut(
    app: &tauri::AppHandle,
    shortcut: &str,
) -> Result<(), tauri_plugin_global_shortcut::Error> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| match event.state {
            ShortcutState::Pressed => pipeline::handle_ptt_press(app),
            ShortcutState::Released => pipeline::handle_ptt_release(app),
        })
}
