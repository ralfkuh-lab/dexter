//! Fensterplatzierung für das Orb-Hauptfenster.

use crate::AppState;
use tauri::Manager;

/// Bring the Orb window back into view: apply config geometry, anchor to the
/// bottom-right of the current monitor if no explicit position is stored,
/// then show + focus.
pub fn reveal_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let win_cfg = {
        let state = app.state::<AppState>();
        let cfg = state.config.lock().unwrap();
        cfg.window.clone()
    };

    let _ = window.set_decorations(win_cfg.decorations);
    let _ = window.set_size(tauri::LogicalSize::new(win_cfg.width, win_cfg.height));

    if let (Some(x), Some(y)) = (win_cfg.x, win_cfg.y) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    } else if let Ok(Some(monitor)) = window.current_monitor() {
        let screen = monitor.size();
        let scale = monitor.scale_factor();
        let padding = 20.0 * scale;
        let bottom_reserved = 80.0 * scale;
        let physical_w = win_cfg.width * scale;
        let physical_h = win_cfg.height * scale;
        let x = screen.width as f64 - physical_w - padding;
        let y = screen.height as f64 - physical_h - padding - bottom_reserved;
        let _ = window.set_position(tauri::PhysicalPosition::new(
            x.max(0.0) as i32,
            y.max(0.0) as i32,
        ));
    }

    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}
