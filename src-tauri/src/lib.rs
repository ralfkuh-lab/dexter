use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    Emitter, Manager,
};
use tokio_util::sync::CancellationToken;

mod agent_session;
mod automation;
mod backend;
mod command_parser;
mod commands;
mod conversation;
mod config;
mod dialog_manager;
mod panel_manager;
mod pipeline;
mod rag;
mod sandbox;
mod state;
mod tool_executor;
mod tools;
mod voice;
mod window;

pub use config::{core_system_prompt, ToolsConfig, VoiceConfig, WindowConfig};
pub use state::{AppState, ChatMessage};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            app_mode: Mutex::new(state::AppMode::default()),
            messages: Mutex::new(Vec::new()),
            config: Mutex::new(VoiceConfig::load()),
            ui_state: Mutex::new(state::UiState::default()),
            pending_dialog: Mutex::new(None),
            processing: Mutex::new(state::ProcessingState::default()),
            automation_events: Mutex::new(Vec::new()),
            console_errors: Mutex::new(Vec::new()),
            rag_store: rag::RagStore::new().expect("Failed to initialize RAG store"),
            audit_log: Mutex::new(sandbox::AuditLog::new()),
            recorded_samples: Mutex::new(Vec::new()),
            recording_sample_rate: Mutex::new(44100),
            is_recording: Mutex::new(false),
            pipeline_cancel: Mutex::new(CancellationToken::new()),
            ctx_max: Mutex::new(None),
            last_stats: Mutex::new(None),
        })
        .setup(|app| {
            let show_item = MenuItemBuilder::with_id("show", "Show/Hide Window").build(app)?;
            let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let text_input_item =
                MenuItemBuilder::with_id("text_input", "Text-Eingabe …").build(app)?;
            let tts_toggle_item =
                MenuItemBuilder::with_id("tts_toggle", "Sprachausgabe umschalten").build(app)?;
            let clear_item = MenuItemBuilder::with_id("clear", "Clear Chat").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&text_input_item)
                .item(&tts_toggle_item)
                .item(&settings_item)
                .item(&clear_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip(format!(
                    "Voice Assistant — Hold {} to talk",
                    app.state::<AppState>().config.lock().unwrap().hotkey
                ))
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                window::reveal_main_window(app);
                            }
                        } else {
                            window::reveal_main_window(app);
                        }
                    }
                    "settings" => {
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        } else {
                            let url = tauri::WebviewUrl::App("index.html?view=settings".into());
                            let _ = WebviewWindowBuilder::new(app, "settings", url)
                                .title("Voice Assistant — Settings")
                                .inner_size(720.0, 700.0)
                                .min_inner_size(600.0, 500.0)
                                .resizable(true)
                                .build();
                        }
                    }
                    "text_input" => {
                        window::reveal_main_window(app);
                        let _ = app.emit("focus_text_input", ());
                    }
                    "tts_toggle" => {
                        let new_value = {
                            let state = app.state::<AppState>();
                            let mut cfg = state.config.lock().unwrap();
                            cfg.tts_enabled = !cfg.tts_enabled;
                            cfg.save();
                            cfg.tts_enabled
                        };
                        let _ = app.emit("config_changed", ());
                        let _ = app.emit("tts_toggled", new_value);
                    }
                    "clear" => {
                        let state = app.state::<AppState>();
                        state.messages.lock().unwrap().clear();
                        let _ = app.emit("messages_cleared", ());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Register global PTT shortcut from config so it works when window is hidden.
            let initial_hotkey = app
                .state::<AppState>()
                .config
                .lock()
                .unwrap()
                .hotkey
                .clone();
            backend::register_ptt_shortcut(app.handle(), &initial_hotkey)?;
            automation::start(app.handle().clone());

            // Probe the LLM backend for max context window (non-blocking).
            backend::refresh_ctx_max(app.handle());

            // Prime the prompt cache so the first real request isn't paying
            // prompt-eval cost for the static system+developer prefix.
            backend::warmup_llm_async(app.handle());

            // Hide dock icon on macOS
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Make webview background transparent and hide on launch.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
                let initial_decorations = app
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .window
                    .decorations;
                let _ = window.set_decorations(initial_decorations);
                let _ = window.hide();

                // Persist geometry on resize/move (debounced via the OS event coalescing).
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Resized(size) => {
                        if let Some(win) = app_handle.get_webview_window("main") {
                            let scale = win.scale_factor().unwrap_or(1.0);
                            let logical = size.to_logical::<f64>(scale);
                            let state = app_handle.state::<AppState>();
                            let mut cfg = state.config.lock().unwrap();
                            if (cfg.window.width - logical.width).abs() > 0.5
                                || (cfg.window.height - logical.height).abs() > 0.5
                            {
                                cfg.window.width = logical.width;
                                cfg.window.height = logical.height;
                                cfg.save();
                            }
                        }
                    }
                    tauri::WindowEvent::Moved(pos) => {
                        let state = app_handle.state::<AppState>();
                        let mut cfg = state.config.lock().unwrap();
                        if cfg.window.x != Some(pos.x) || cfg.window.y != Some(pos.y) {
                            cfg.window.x = Some(pos.x);
                            cfg.window.y = Some(pos.y);
                            cfg.save();
                        }
                    }
                    _ => {}
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_mode,
            commands::get_config,
            commands::get_core_system_prompt,
            commands::get_ctx_max,
            commands::get_last_stats,
            commands::get_panel_content,
            commands::resolve_dialog,
            commands::list_models,
            commands::set_config,
            commands::get_messages,
            commands::clear_messages,
            commands::show_window,
            commands::hide_window,
            commands::ingest_text,
            commands::ingest_file,
            commands::list_knowledge_sources,
            commands::delete_knowledge_source,
            commands::start_recording,
            commands::stop_recording_and_process,
            commands::set_tts_enabled,
            commands::submit_text,
            commands::get_system_info,
            commands::automation_console_error,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
