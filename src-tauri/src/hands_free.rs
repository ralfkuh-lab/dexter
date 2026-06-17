//! Hands-free conversation mode: continuous microphone input with direct turns.

use crate::state::{emit_processing, ProcessingState};
use crate::AppState;
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

pub fn activate(app: &tauri::AppHandle) {
    if crate::dictation::is_active(app) {
        crate::dictation::deactivate(app);
    }

    {
        let state = app.state::<AppState>();
        *state.is_recording.lock().unwrap() = false;
        *state.hands_free_active.lock().unwrap() = true;
        state.dictation_buffer.lock().unwrap().clear();
    }
    let _ = app.emit("hands_free_mode_changed", true);
    let _ = app.emit("dictation_buffer_updated", "");
    crate::agent_draft::reset_for_current_mode(app);
    if *app.state::<AppState>().app_mode.lock().unwrap() != crate::state::AppMode::Chat {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = crate::agent_draft::show_window(&app_handle).await;
        });
    }
    let _ = emit_processing(
        app,
        ProcessingState {
            stage: "listening".to_string(),
            text: "Höre zu...".to_string(),
        },
    );
    crate::pipeline::start_hands_free_loop(app);
}

pub fn deactivate(app: &tauri::AppHandle) {
    crate::pipeline::stop_hands_free_loop(app);
    {
        let state = app.state::<AppState>();
        *state.hands_free_active.lock().unwrap() = false;
    }
    let _ = app.emit("hands_free_mode_changed", false);
    let _ = app.emit(
        "dictation_vad",
        serde_json::json!({ "rms": 0.0, "threshold": 0.0, "speech": false }),
    );
    let _ = emit_processing(
        app,
        ProcessingState {
            stage: "idle".to_string(),
            text: String::new(),
        },
    );
}

pub fn is_active(app: &tauri::AppHandle) -> bool {
    *app.state::<AppState>().hands_free_active.lock().unwrap()
}

pub fn pipeline_cancel_token(app: &tauri::AppHandle) -> CancellationToken {
    let state = app.state::<AppState>();
    let mut cancel = state.pipeline_cancel.lock().unwrap();
    cancel.cancel();
    *cancel = CancellationToken::new();
    cancel.clone()
}
