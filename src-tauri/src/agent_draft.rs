//! Agent draft window and LLM-controlled prompt drafting for hands-free agent input.

use crate::state::{emit_processing, AgentDraftInfo, ProcessingState};
use crate::{voice, AppState, ChatMessage, VoiceConfig};
use serde_json::Value;
use tauri::{Emitter, Manager, WebviewWindowBuilder};

const CONTROLLER_TOOL: &str = "update_agent_draft";

pub fn current(app: &tauri::AppHandle) -> AgentDraftInfo {
    app.state::<AppState>()
        .ui_state
        .lock()
        .unwrap()
        .agent_draft
        .clone()
}

pub fn set_content(app: &tauri::AppHandle, content: String) {
    update_info(app, |draft| {
        draft.content = content;
        draft.status = "editing".to_string();
    });
}

pub fn clear(app: &tauri::AppHandle) {
    update_info(app, |draft| {
        draft.content.clear();
        draft.spoken_log.clear();
        draft.last_segment.clear();
        draft.status = "empty".to_string();
    });
}

pub fn reset_for_current_mode(app: &tauri::AppHandle) {
    let mode = app.state::<AppState>().app_mode.lock().unwrap().to_string();
    update_info(app, |draft| {
        draft.mode = mode;
        draft.content.clear();
        draft.spoken_log.clear();
        draft.last_segment.clear();
        draft.status = "empty".to_string();
    });
}

pub async fn show_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window("agent_draft") {
        window
    } else {
        let url = tauri::WebviewUrl::App("index.html?view=agent-draft".into());
        WebviewWindowBuilder::new(app, "agent_draft", url)
            .title("Dexter Agent Draft")
            .inner_size(1100.0, 720.0)
            .min_inner_size(860.0, 560.0)
            .resizable(true)
            .decorations(true)
            .center()
            .build()
            .map_err(|e| e.to_string())?
    };

    let _ = window.set_size(tauri::LogicalSize::new(1100.0, 720.0));
    let _ = window.center();
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let _ = window.center();
    emit_draft(app);
    Ok(())
}

pub async fn process_segment(
    app: &tauri::AppHandle,
    segment: &str,
    config: &VoiceConfig,
) -> Result<(), String> {
    let mode = app.state::<AppState>().app_mode.lock().unwrap().to_string();
    update_info(app, |draft| {
        draft.mode = mode;
        draft.last_segment = segment.trim().to_string();
        if !segment.trim().is_empty() {
            draft.spoken_log.push(segment.trim().to_string());
            if draft.spoken_log.len() > 20 {
                let excess = draft.spoken_log.len() - 20;
                draft.spoken_log.drain(0..excess);
            }
        }
        draft.status = "formuliere prompt".to_string();
    });
    record_spoken_segment(app, segment)?;
    show_window(app).await?;

    let action = match interpret_segment(app, segment, config).await {
        Ok(action) => action,
        Err(e) => {
            eprintln!(
                "Draft controller failed, keeping transcript as draft: {}",
                e
            );
            update_info(app, |draft| {
                draft.status = "controller fallback".to_string();
            });
            DraftAction::ReplaceDraft(fallback_prompt(app, segment))
        }
    };
    apply_action(app, action).await
}

pub async fn submit(app: &tauri::AppHandle) -> Result<(), String> {
    let (mode, text) = {
        let state = app.state::<AppState>();
        let mode = state.app_mode.lock().unwrap().clone();
        let text = {
            let ui = state.ui_state.lock().unwrap();
            ui.agent_draft.content.trim().to_string()
        };
        (mode, text)
    };

    if text.is_empty() {
        update_info(app, |draft| {
            draft.status = "empty".to_string();
        });
        return Ok(());
    }

    send_to_agent(app, &mode, &text).await?;

    clear(app);
    update_info(app, |draft| {
        draft.mode = mode.to_string();
        draft.status = "sent".to_string();
    });
    Ok(())
}

async fn interpret_segment(
    app: &tauri::AppHandle,
    segment: &str,
    config: &VoiceConfig,
) -> Result<DraftAction, String> {
    let draft = current(app);
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: controller_prompt(),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: format!(
                "Current draft:\n{}\n\nNew speech transcript:\n{}",
                if draft.content.trim().is_empty() {
                    "<empty>"
                } else {
                    draft.content.trim()
                },
                segment.trim()
            ),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let (sentence_tx, mut sentence_rx) = tokio::sync::mpsc::channel::<String>(32);
    let drain_handle = tokio::spawn(async move { while sentence_rx.recv().await.is_some() {} });
    let result = voice::chat_streaming(
        app,
        config,
        &messages,
        &controller_tools(),
        Some(CONTROLLER_TOOL),
        &sentence_tx,
    )
    .await
    .map_err(|e| format!("Draft controller failed: {}", e))?;
    drop(sentence_tx);
    drain_handle.abort();

    match result {
        voice::StreamResult::ToolCalls { calls, .. } => Ok(calls
            .first()
            .map(|call| action_from_tool_call(call, app, segment))
            .unwrap_or_else(|| DraftAction::ReplaceDraft(fallback_prompt(app, segment)))),
        voice::StreamResult::Content(content) => Ok(DraftAction::ReplaceDraft(
            sanitize_content_draft(&content).unwrap_or_else(|| fallback_prompt(app, segment)),
        )),
    }
}

fn action_from_tool_call(
    call: &voice::ToolCall,
    app: &tauri::AppHandle,
    original_segment: &str,
) -> DraftAction {
    let action = call
        .function
        .arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("replace_draft");
    let text = call
        .function
        .arguments
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    match action {
        "replace_draft" | "replace_text" | "append_text" => DraftAction::ReplaceDraft(
            sanitize_content_draft(&text).unwrap_or_else(|| fallback_prompt(app, original_segment)),
        ),
        "clear_draft" => DraftAction::ClearDraft,
        "submit_draft" => DraftAction::SubmitDraft,
        "noop" => DraftAction::Noop,
        _ => DraftAction::ReplaceDraft(
            sanitize_content_draft(&text).unwrap_or_else(|| fallback_prompt(app, original_segment)),
        ),
    }
}

async fn apply_action(app: &tauri::AppHandle, action: DraftAction) -> Result<(), String> {
    match action {
        DraftAction::ReplaceDraft(text) => {
            update_info(app, |draft| {
                draft.content = text;
                draft.status = "prompt entworfen".to_string();
            });
        }
        DraftAction::ClearDraft => clear(app),
        DraftAction::SubmitDraft => submit(app).await?,
        DraftAction::Noop => update_info(app, |draft| {
            draft.status = "ready".to_string();
        }),
    }
    Ok(())
}

fn update_info(app: &tauri::AppHandle, f: impl FnOnce(&mut AgentDraftInfo)) {
    {
        let state = app.state::<AppState>();
        let mut ui = state.ui_state.lock().unwrap();
        f(&mut ui.agent_draft);
    }
    emit_draft(app);
}

fn emit_draft(app: &tauri::AppHandle) {
    let draft = current(app);
    let _ = app.emit("agent_draft_updated", &draft);
    let _ = app.emit_to("agent_draft", "agent_draft_updated", draft);
}

async fn send_to_agent(
    app: &tauri::AppHandle,
    mode: &crate::state::AppMode,
    prompt: &str,
) -> Result<(), String> {
    use crate::agent_session;
    use crate::state::record_automation_event;

    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let session = agent_session::ensure_session(mode, &working_dir).await?;
    let terminal_command = app
        .state::<AppState>()
        .config
        .lock()
        .unwrap()
        .terminal_command
        .clone();

    agent_session::open_terminal(&session.name, &terminal_command).await?;
    if session.created {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    agent_session::send_keys(&session.pane_id, prompt).await?;

    record_automation_event(
        app,
        "agent.sent",
        &format!("{}:{}", mode, truncate_chars(prompt, 80)),
    );
    let _ = app.emit("assistant_text", &format!("➜ {} — Draft gesendet", mode));
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DraftAction {
    ReplaceDraft(String),
    ClearDraft,
    SubmitDraft,
    Noop,
}

fn controller_prompt() -> String {
    "You are Dexter's internal prompt editor for a hands-free coding-agent session. The user speaks casually in German; transcripts may contain recognition errors, corrections, afterthoughts, and meta instructions such as \"sag Claude mal ...\", \"nimm das wieder raus\", or \"frag ihn nach seiner Meinung\". Your job is to maintain one polished prompt that will be sent to the coding agent, not to copy the transcript verbatim.\n\nAlways call update_agent_draft with exactly one action.\n- Use replace_draft for ordinary speech, additions, removals, corrections, or reformulations. The text must be the complete updated prompt for the coding agent.\n- Use submit_draft only when the user clearly approves sending now, e.g. \"sende den Prompt ab\", \"okay, abschicken\", \"schick das jetzt an Claude/Codex/agy\".\n- Use clear_draft only when the user clearly wants to discard the whole prompt.\n- Use noop only for pure noise or irrelevant filler.\n\nWrite the draft prompt in German unless the user explicitly asks otherwise. Make it concrete, concise, and actionable for a coding agent. Preserve important user intent, remove conversational filler, and incorporate corrections by rewriting the whole prompt. Do not answer in prose outside the tool call.".to_string()
}

fn controller_tools() -> Vec<serde_json::Value> {
    vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": CONTROLLER_TOOL,
            "description": "Update or submit the polished prompt draft for the current coding agent.",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "replace_draft",
                            "clear_draft",
                            "submit_draft",
                            "noop"
                        ]
                    },
                    "text": {
                        "type": "string",
                        "description": "The complete updated coding-agent prompt for replace_draft. Omit for other actions."
                    }
                },
                "required": ["action"]
            }
        }
    })]
}

fn sanitize_content_draft(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_prefix = trimmed
        .strip_prefix("Prompt:")
        .or_else(|| trimmed.strip_prefix("Draft:"))
        .unwrap_or(trimmed)
        .trim();
    if without_prefix.is_empty() {
        None
    } else {
        Some(without_prefix.to_string())
    }
}

fn fallback_prompt(app: &tauri::AppHandle, segment: &str) -> String {
    let current = current(app).content.trim().to_string();
    if current.is_empty() {
        format!(
            "Bitte formuliere aus dieser Sprachangabe einen geeigneten Arbeitsauftrag:\n\n{}",
            segment.trim()
        )
    } else {
        format!(
            "{}\n\nWeitere Nutzerangabe, die noch eingearbeitet werden muss:\n{}",
            current,
            segment.trim()
        )
    }
}

fn record_spoken_segment(app: &tauri::AppHandle, segment: &str) -> Result<(), String> {
    let text = segment.trim();
    if text.is_empty() {
        return Ok(());
    }
    app.state::<AppState>()
        .messages
        .lock()
        .unwrap()
        .push(ChatMessage {
            role: "user".to_string(),
            content: text.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
    emit_processing(
        app,
        ProcessingState {
            stage: "transcribed".to_string(),
            text: text.to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}
