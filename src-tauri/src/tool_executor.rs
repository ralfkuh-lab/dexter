//! Dispatch und Ausführung von LLM-Tool-Calls.

use crate::dialog_manager::clear_pending_dialog;
use crate::panel_manager::show_panel;
use crate::state::{
    emit_processing, record_automation_event, AppMode, DialogOption, DialogPayload, ProcessingState,
};
use crate::{agent_session, sandbox, tools, voice, AppState, ToolsConfig, VoiceConfig};
use tauri::{Emitter, Manager};

pub async fn execute_tool(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    tool_call: &voice::OllamaToolCall,
) -> String {
    match tool_call.function.name.as_str() {
        "search_notes" => {
            let query = tool_call
                .function
                .arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            tools::search_notes(&config.vault_path, &query)
        }
        "read_note" => {
            let path = tool_call
                .function
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            match tools::read_note(&config.vault_path, &path) {
                Ok(text) => text,
                Err(e) => e,
            }
        }
        "take_screenshot" => {
            let question = tool_call
                .function
                .arguments
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("Describe what you see on this screen in detail.")
                .to_string();
            let monitor = tool_call
                .function
                .arguments
                .get("monitor")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);

            let _ = emit_processing(
                app,
                ProcessingState {
                    stage: "thinking".to_string(),
                    text: "Looking at your screen...".to_string(),
                },
            );

            match tools::take_screenshot(monitor) {
                Ok(image_b64) => {
                    let vision_model = if config.vision_model.is_empty() {
                        &config.llm_model
                    } else {
                        &config.vision_model
                    };
                    match tools::describe_screenshot(
                        &config.llm_base_url,
                        &config.llm_provider,
                        vision_model,
                        &image_b64,
                        &question,
                    )
                    .await
                    {
                        Ok(desc) => desc,
                        Err(e) => format!(
                            "Screenshot captured but vision model failed: {}. \
                             The model '{}' may not support image inputs — \
                             try setting a vision model like 'llava' in settings.",
                            e, vision_model
                        ),
                    }
                }
                Err(e) => format!("Failed to capture screenshot: {}", e),
            }
        }
        "read_clipboard" => match tools::read_clipboard() {
            Ok(text) => {
                if text.trim().is_empty() {
                    "The clipboard is empty.".to_string()
                } else {
                    format!("Clipboard contents:\n{}", text)
                }
            }
            Err(e) => format!("Failed to read clipboard: {}", e),
        },
        "open_url" => {
            let url = tool_call
                .function
                .arguments
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if url.is_empty() {
                "No URL provided.".to_string()
            } else {
                match tools::open_url(&url) {
                    Ok(msg) => msg,
                    Err(e) => format!("Failed to open URL: {}", e),
                }
            }
        }
        "get_current_time" => tools::get_current_time(),
        "list_running_apps" => match tools::list_running_apps() {
            Ok(apps) => format!("Currently running applications:\n{}", apps),
            Err(e) => format!("Failed to list apps: {}", e),
        },
        "web_fetch" => {
            let url = tool_call
                .function
                .arguments
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if url.is_empty() {
                "No URL provided.".to_string()
            } else {
                match tools::web_fetch(&url).await {
                    Ok(text) => text,
                    Err(e) => format!("Failed to fetch {}: {}", url, e),
                }
            }
        }
        "show_panel" => {
            let title = tool_call
                .function
                .arguments
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Details")
                .trim()
                .to_string();
            let content = tool_call
                .function
                .arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = if title.is_empty() {
                "Details".to_string()
            } else {
                title
            };

            match show_panel(app, title.clone(), content).await {
                Ok(()) => format!("Panel '{}' geöffnet.", title),
                Err(e) => format!("Failed to open panel: {}", e),
            }
        }
        "ask_user" => {
            let question = tool_call
                .function
                .arguments
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("Welche Option soll ich nehmen?")
                .trim()
                .to_string();
            let options = tool_call
                .function
                .arguments
                .get("options")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            let label = item
                                .get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .trim();
                            if label.is_empty() {
                                return None;
                            }
                            let description = item
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(ToString::to_string);
                            Some(DialogOption {
                                label: label.to_string(),
                                description,
                            })
                        })
                        .take(4)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            if options.len() < 2 {
                return "ask_user failed: at least two options are required.".to_string();
            }

            let question = if question.is_empty() {
                "Welche Option soll ich nehmen?".to_string()
            } else {
                question
            };
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            {
                let state = app.state::<AppState>();
                if let Some(existing) = state.pending_dialog.lock().unwrap().take() {
                    let _ = existing
                        .responder
                        .send("No selection received.".to_string());
                }
                *state.pending_dialog.lock().unwrap() = Some(crate::state::DialogState {
                    question: question.clone(),
                    options: options.clone(),
                    responder: tx,
                });
            }

            let payload = DialogPayload {
                question: question.clone(),
                options: options.clone(),
            };
            let _ = app.emit("show_dialog", payload);

            match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                Ok(Ok(selected)) => format!("User selected: {}", selected),
                Ok(Err(_)) => "No selection received.".to_string(),
                Err(_) => {
                    clear_pending_dialog(app, &question);
                    let _ = app.emit("dismiss_dialog", ());
                    "No selection received before timeout.".to_string()
                }
            }
        }
        "switch_mode" => {
            let mode_str = tool_call
                .function
                .arguments
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("chat");

            let mode: AppMode =
                serde_json::from_value(serde_json::Value::String(mode_str.to_string()))
                    .unwrap_or(AppMode::Chat);

            let label = mode.to_string();
            {
                let state = app.state::<AppState>();
                *state.app_mode.lock().unwrap() = mode.clone();
            }
            let _ = app.emit("app_mode_changed", &label);
            record_automation_event(app, "mode.changed", &label);

            if mode != AppMode::Chat {
                let working_dir =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                match agent_session::ensure_session(&mode, &working_dir).await {
                    Ok(session) => {
                        let _ = agent_session::open_terminal(&session.name).await;
                        format!("Modus auf {} gewechselt. Terminal geöffnet.", label)
                    }
                    Err(e) => format!(
                        "Modus auf {} gewechselt, aber Terminal-Start fehlgeschlagen: {}",
                        label, e
                    ),
                }
            } else {
                format!("Zurück im Chat-Modus.")
            }
        }
        "run_command" => {
            let command = tool_call
                .function
                .arguments
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if command.is_empty() {
                "No command provided.".to_string()
            } else {
                let _ = emit_processing(
                    app,
                    ProcessingState {
                        stage: "thinking".to_string(),
                        text: format!("Running: {}", command),
                    },
                );
                let audit = &app.state::<AppState>().audit_log;
                match sandbox::execute(&command, &config.sandbox, audit) {
                    Ok(output) => output,
                    Err(e) => format!("Sandbox: {}", e),
                }
            }
        }
        unknown => format!("Unknown tool: {}", unknown),
    }
}

pub fn forced_tool_for_transcript(transcript: &str, tools: &ToolsConfig) -> Option<String> {
    let text = transcript.to_lowercase();

    if tools.get_current_time
        && contains_any(
            &text,
            &[
                "wie spät",
                "wieviel uhr",
                "wie viel uhr",
                "uhrzeit",
                "aktuelle zeit",
                "aktuelles datum",
                "welches datum",
                "welcher tag",
                "heutiges datum",
            ],
        )
    {
        return Some("get_current_time".to_string());
    }

    if tools.read_clipboard
        && contains_any(
            &text,
            &[
                "zwischenablage",
                "clipboard",
                "kopiert",
                "was habe ich kopiert",
                "was ist im clipboard",
                "lies clipboard",
                "lies die zwischenablage",
            ],
        )
    {
        return Some("read_clipboard".to_string());
    }

    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}
