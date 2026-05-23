//! Panel-Fenster und einfache UI-Sprachbefehle (schließen etc.).

use crate::state::PanelInfo;
use crate::AppState;
use tauri::{Emitter, Manager, WebviewWindowBuilder};

pub async fn show_panel(
    app: &tauri::AppHandle,
    title: String,
    content: String,
) -> Result<(), String> {
    let panel_info = PanelInfo {
        title: title.clone(),
        content: content.clone(),
    };

    {
        let state = app.state::<AppState>();
        state.ui_state.lock().unwrap().panel = Some(panel_info.clone());
    }

    let window = if let Some(window) = app.get_webview_window("panel") {
        window
    } else {
        let url = tauri::WebviewUrl::App("index.html?view=panel".into());
        let window = WebviewWindowBuilder::new(app, "panel", url)
            .title(format!("Dexter - {}", title))
            .inner_size(600.0, 500.0)
            .min_inner_size(400.0, 300.0)
            .resizable(true)
            .decorations(true)
            .build()
            .map_err(|e| e.to_string())?;

        let app_handle = app.clone();
        window.on_window_event(move |event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                let state = app_handle.state::<AppState>();
                state.ui_state.lock().unwrap().panel = None;
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        window
    };

    let _ = window.set_title(&format!("Dexter - {}", title));
    let _ = window.show();
    let _ = window.set_focus();
    app.emit_to("panel", "panel_content", panel_info)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn handle_ui_command(app: &tauri::AppHandle, transcript: &str) -> bool {
    let text = transcript.to_lowercase();
    let close_words = [
        "schließ",
        "schliess",
        "schließe",
        "schliesse",
        "close",
        "panel zu",
        "fenster zu",
        "mach zu",
        "mach das panel zu",
        "schließ das panel",
        "schliess das panel",
        "ok danke",
    ];

    if close_words.iter().any(|w| text.contains(w)) {
        let state = app.state::<AppState>();
        let had_panel = state.ui_state.lock().unwrap().panel.is_some();
        if had_panel {
            if let Some(window) = app.get_webview_window("panel") {
                let _ = window.close();
            }
            state.ui_state.lock().unwrap().panel = None;
            return true;
        }
    }

    false
}

pub fn build_ui_context(app: &tauri::AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let ui = state.ui_state.lock().unwrap();
    let mut parts = Vec::new();

    if let Some(panel) = ui.panel.as_ref() {
        parts.push(format!("Detail panel '{}' is open.", panel.title));
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("[UI state: {}]", parts.join(" ")))
    }
}
