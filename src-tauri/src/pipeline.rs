//! Audio-→-STT-→-LLM-→-TTS-Pipeline plus PTT-Handler und Tool-Ausführung.

use crate::state::{
    emit_processing, update_processing_state, AudioChunk, DialogOption, DialogPayload, PanelInfo,
    ProcessingState,
};
use crate::window::reveal_main_window;
use crate::{sandbox, tools, voice, AppState, ChatMessage, ToolsConfig, VoiceConfig};
use tauri::{Emitter, Manager, WebviewWindowBuilder};
use tokio_util::sync::CancellationToken;

pub fn handle_ptt_press(app: &tauri::AppHandle) {
    {
        let state = app.state::<AppState>();
        let dialog_pending = state.pending_dialog.lock().unwrap().is_some();
        if !dialog_pending {
            let mut cancel = state.pipeline_cancel.lock().unwrap();
            cancel.cancel();
            *cancel = CancellationToken::new();
        }
    }

    if !has_pending_dialog(app) {
        let _ = app.emit("pipeline_interrupted", ());
    }
    reveal_main_window(app);
    update_processing_state(
        app,
        ProcessingState {
            stage: "listening".to_string(),
            text: String::new(),
        },
    );
    let _ = app.emit("hotkey_pressed", ());

    let state = app.state::<AppState>();
    let is_rec = *state.is_recording.lock().unwrap();
    if !is_rec {
        state.recorded_samples.lock().unwrap().clear();
        *state.is_recording.lock().unwrap() = true;
        let app_clone = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = voice::record_audio(&app_clone) {
                eprintln!("Recording error: {}", e);
            }
        });
    }
}

pub fn handle_ptt_release(app: &tauri::AppHandle) {
    update_processing_state(
        app,
        ProcessingState {
            stage: "transcribing".to_string(),
            text: String::new(),
        },
    );
    let _ = app.emit("hotkey_released", ());

    let state = app.state::<AppState>();
    *state.is_recording.lock().unwrap() = false;
    let cancel_token = state.pipeline_cancel.lock().unwrap().clone();

    let app_clone = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));

        let state = app_clone.state::<AppState>();
        let samples = state.recorded_samples.lock().unwrap().clone();
        let sample_rate = *state.recording_sample_rate.lock().unwrap();
        let config = state.config.lock().unwrap().clone();

        if samples.is_empty() {
            let _ = emit_processing(
                &app_clone,
                ProcessingState {
                    stage: "idle".to_string(),
                    text: String::new(),
                },
            );
            return;
        }

        tauri::async_runtime::spawn(async move {
            if let Err(e) = process_pipeline(
                app_clone.clone(),
                samples,
                sample_rate,
                config,
                cancel_token,
            )
            .await
            {
                if e != "interrupted" {
                    eprintln!("Pipeline error: {}", e);
                    let _ = emit_processing(
                        &app_clone,
                        ProcessingState {
                            stage: "error".to_string(),
                            text: e,
                        },
                    );
                }
            }
        });
    });
}

pub async fn process_pipeline(
    app: tauri::AppHandle,
    samples: Vec<f32>,
    sample_rate: u32,
    config: VoiceConfig,
    cancel: CancellationToken,
) -> Result<(), String> {
    // Stage 1: Transcribe
    emit_processing(
        &app,
        ProcessingState {
            stage: "transcribing".to_string(),
            text: "Transcribing...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let transcript =
        voice::transcribe_audio_http(&config.whisper_server_url, &samples, sample_rate)
            .await
            .map_err(|e| format!("Transcription failed: {}", e))?;

    if cancel.is_cancelled() {
        return Err("interrupted".to_string());
    }

    if transcript.trim().is_empty() {
        emit_processing(
            &app,
            ProcessingState {
                stage: "idle".to_string(),
                text: String::new(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
        return Ok(());
    }

    run_llm_pipeline(app, transcript, config, cancel).await
}

/// Pipeline-Variante für Texteingabe: überspringt STT komplett, ruft den
/// gemeinsamen LLM-/TTS-Kern direkt mit dem User-Text auf.
pub async fn process_text_input(
    app: tauri::AppHandle,
    text: String,
    config: VoiceConfig,
    cancel: CancellationToken,
) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("No text provided".to_string());
    }
    run_llm_pipeline(app, text, config, cancel).await
}

/// Gemeinsamer Kern: nimmt einen User-Text (egal ob aus STT oder Tastatur),
/// hängt ihn an die Chat-Historie, fährt die LLM-Schleife mit Tools, streamt
/// Sätze an TTS und gibt die Audio-Chunks ans Frontend.
async fn run_llm_pipeline(
    app: tauri::AppHandle,
    transcript: String,
    config: VoiceConfig,
    cancel: CancellationToken,
) -> Result<(), String> {
    emit_processing(
        &app,
        ProcessingState {
            stage: "transcribed".to_string(),
            text: transcript.clone(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    if handle_pending_dialog_answer(&app, &transcript) {
        emit_processing(
            &app,
            ProcessingState {
                stage: "idle".to_string(),
                text: String::new(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
        return Ok(());
    }

    if let Some(cmd) = crate::command_parser::parse(&transcript) {
        handle_command(&app, cmd);
        emit_processing(
            &app,
            ProcessingState {
                stage: "idle".to_string(),
                text: String::new(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
        return Ok(());
    }

    if handle_ui_command(&app, &transcript) {
        emit_processing(
            &app,
            ProcessingState {
                stage: "idle".to_string(),
                text: String::new(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
        return Ok(());
    }

    {
        let mode = app.state::<AppState>().app_mode.lock().unwrap().clone();
        if mode != crate::state::AppMode::Chat {
            let msg = format!(
                "Session-Routing für {} ist noch nicht implementiert. \
                 Sage \"Kommando Chat\" um zurück in den Chat-Modus zu wechseln.",
                mode
            );
            let _ = app.emit("assistant_text", &msg);
            emit_processing(
                &app,
                ProcessingState {
                    stage: "idle".to_string(),
                    text: String::new(),
                },
            )
            .map_err(|e: tauri::Error| e.to_string())?;
            return Ok(());
        }
    }

    // Add user message
    {
        app.state::<AppState>()
            .messages
            .lock()
            .unwrap()
            .push(ChatMessage {
                role: "user".to_string(),
                content: transcript.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
    }

    // Stage 2: LLM with tool calling → streaming TTS
    emit_processing(
        &app,
        ProcessingState {
            stage: "thinking".to_string(),
            text: "Thinking...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let mut all_messages =
        redact_stale_tool_results(&app.state::<AppState>().messages.lock().unwrap());
    if let Some(ui_context) = build_ui_context(&app) {
        let insert_at = all_messages
            .iter()
            .rposition(|m| m.role == "user")
            .unwrap_or(all_messages.len());
        all_messages.insert(
            insert_at,
            ChatMessage {
                role: "system".to_string(),
                content: ui_context,
                tool_calls: None,
                tool_call_id: None,
            },
        );
    }

    let tools = voice::build_tools(&config.tools);
    let forced_tool = forced_tool_for_transcript(&transcript, &config.tools);
    let max_tool_rounds = 5;

    // Single streaming loop: stream with tools → if model returns tool calls,
    // execute them and stream again. If it returns content, sentences flow to TTS.
    let (sentence_tx, mut sentence_rx) = tokio::sync::mpsc::channel::<String>(16);
    let mut sentence_index: u32 = 0;
    let mut full_text = String::new();

    let app_clone = app.clone();
    let config_clone = config.clone();
    let cancel_llm = cancel.clone();

    let llm_handle = {
        let tools = tools.clone();
        let sentence_tx = sentence_tx.clone();
        let app = app_clone.clone();
        let config = config_clone.clone();

        tokio::spawn(async move {
            let mut all_msgs = all_messages;
            let mut forced_tool_next = forced_tool;

            for _round in 0..max_tool_rounds {
                if cancel_llm.is_cancelled() {
                    return Err("interrupted".to_string());
                }

                if config.debug_bubbles {
                    let _ = app.emit(
                        "llm_debug",
                        format!(
                            "LLM request: provider={}, model={}, messages={}, tools={}, forced_tool={}",
                            config.llm_provider,
                            config.llm_model,
                            all_msgs.len(),
                            tools.len(),
                            forced_tool_next.as_deref().unwrap_or("none")
                        ),
                    );
                }

                let forced_tool_this_round = forced_tool_next.take();
                let result = tokio::select! {
                    _ = cancel_llm.cancelled() => { return Err("interrupted".to_string()); }
                    r = voice::chat_streaming(&app, &config, &all_msgs, &tools, forced_tool_this_round.as_deref(), &sentence_tx) => {
                        r.map_err(|e| format!("LLM failed: {}", e))?
                    }
                };

                match result {
                    voice::StreamResult::Content(text) => {
                        if config.debug_bubbles {
                            let _ = app.emit(
                                "llm_debug",
                                format!(
                                    "LLM response: content only, {} chars",
                                    text.chars().count()
                                ),
                            );
                        }
                        return Ok::<String, String>(text);
                    }
                    voice::StreamResult::ToolCalls {
                        calls: tool_calls,
                        spoken_preamble: preamble,
                        source,
                    } => {
                        if cancel_llm.is_cancelled() {
                            return Err("interrupted".to_string());
                        }
                        let names = tool_calls
                            .iter()
                            .map(|tc| tc.function.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        if config.debug_bubbles {
                            let _ = app.emit(
                                "llm_debug",
                                format!(
                                    "LLM response: tool_calls=[{}], preamble_chars={}, source={:?}",
                                    names,
                                    preamble.chars().count(),
                                    source
                                ),
                            );
                        }

                        if source == voice::ToolCallSource::Xml {
                            // XML-parsed tool calls: model emitted XML as text.
                            // Add the preamble as assistant content, then inject
                            // tool results as a user message (model doesn't understand
                            // native tool protocol).
                            if !preamble.is_empty() {
                                all_msgs.push(ChatMessage {
                                    role: "assistant".to_string(),
                                    content: preamble,
                                    tool_calls: None,
                                    tool_call_id: None,
                                });
                            }

                            let mut tool_results = String::new();
                            for tool_call in &tool_calls {
                                if cancel_llm.is_cancelled() {
                                    return Err("interrupted".to_string());
                                }

                                let _ = emit_processing(
                                    &app,
                                    ProcessingState {
                                        stage: "tool_call".to_string(),
                                        text: tool_call.function.name.clone(),
                                    },
                                );

                                let result_text = execute_tool(&app, &config, tool_call).await;
                                tool_results.push_str(&format!(
                                    "[Tool result for {}]: {}\n",
                                    tool_call.function.name, result_text
                                ));
                            }

                            all_msgs.push(ChatMessage {
                                role: "user".to_string(),
                                content: format!(
                                    "Here are the tool results. Use them to answer my previous question naturally. Do NOT call tools again.\n\n{}",
                                    tool_results.trim()
                                ),
                                tool_calls: None,
                                tool_call_id: None,
                            });
                        } else {
                            // Native tool calls: preserve the assistant call and answer each call with a tool message.
                            let tool_calls_out: Vec<voice::OllamaToolCallOut> =
                                tool_calls.iter().map(|tc| tc.to_out()).collect();
                            all_msgs.push(ChatMessage {
                                role: "assistant".to_string(),
                                content: preamble,
                                tool_calls: Some(tool_calls_out),
                                tool_call_id: None,
                            });

                            for tool_call in &tool_calls {
                                if cancel_llm.is_cancelled() {
                                    return Err("interrupted".to_string());
                                }

                                let _ = emit_processing(
                                    &app,
                                    ProcessingState {
                                        stage: "tool_call".to_string(),
                                        text: tool_call.function.name.clone(),
                                    },
                                );

                                let result_text = execute_tool(&app, &config, tool_call).await;

                                all_msgs.push(ChatMessage {
                                    role: "tool".to_string(),
                                    content: result_text,
                                    tool_calls: None,
                                    tool_call_id: tool_call.id.clone(),
                                });
                            }
                        }

                        let _ = emit_processing(
                            &app,
                            ProcessingState {
                                stage: "thinking".to_string(),
                                text: "Thinking...".to_string(),
                            },
                        );
                    }
                }
            }

            // Hit max rounds — do one final stream without tools
            if cancel_llm.is_cancelled() {
                return Err("interrupted".to_string());
            }

            if config.debug_bubbles {
                let _ = app.emit(
                    "llm_debug",
                    "LLM final request: tools disabled after max rounds",
                );
            }
            let result = voice::chat_streaming(&app, &config, &all_msgs, &[], None, &sentence_tx)
                .await
                .map_err(|e| format!("LLM failed: {}", e))?;

            match result {
                voice::StreamResult::Content(text) => Ok(text),
                voice::StreamResult::ToolCalls { .. } => {
                    Err("Model returned tool calls after max rounds".to_string())
                }
            }
        })
    };

    // Drop our copy of sentence_tx so the channel closes when the spawned task finishes
    drop(sentence_tx);

    // Process sentences as they arrive from the stream → TTS → audio.
    // Check cancellation between each TTS synthesis.
    while let Some(sentence) = sentence_rx.recv().await {
        if cancel.is_cancelled() {
            break;
        }

        full_text.push_str(&sentence);
        full_text.push(' ');

        emit_processing(
            &app,
            ProcessingState {
                stage: "speaking".to_string(),
                text: full_text.trim().to_string(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;

        if !config.tts_enabled {
            // Lautsprecher aus: Text läuft trotzdem satzweise in die Bubble,
            // aber kein TTS-Roundtrip und keine Audio-Chunks. Wir warten kurz,
            // damit die Bubble nicht in einem Frame "explodiert", sondern in
            // gesprochenem Rhythmus mitläuft.
            tokio::select! {
                _ = cancel.cancelled() => { break; }
                _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
            }
            continue;
        }

        let tts_result = tokio::select! {
            _ = cancel.cancelled() => { break; }
            r = voice::synthesize(&config, &sentence) => r
        };

        match tts_result {
            Ok(audio_base64) => {
                if cancel.is_cancelled() {
                    break;
                }
                app.emit(
                    "play_audio_chunk",
                    AudioChunk {
                        index: sentence_index,
                        audio: audio_base64,
                    },
                )
                .map_err(|e: tauri::Error| e.to_string())?;
                sentence_index += 1;
            }
            Err(e) => {
                eprintln!("TTS failed for sentence: {}", e);
            }
        }
    }

    if cancel.is_cancelled() {
        llm_handle.abort();
        return Err("interrupted".to_string());
    }

    let full_response = llm_handle
        .await
        .map_err(|e| format!("LLM task failed: {}", e))??;

    app.emit("play_audio_done", sentence_index)
        .map_err(|e: tauri::Error| e.to_string())?;

    // Add assistant message to history
    app.state::<AppState>()
        .messages
        .lock()
        .unwrap()
        .push(ChatMessage {
            role: "assistant".to_string(),
            content: full_response,
            tool_calls: None,
            tool_call_id: None,
        });

    update_processing_state(
        &app,
        ProcessingState {
            stage: "idle".to_string(),
            text: String::new(),
        },
    );

    Ok(())
}

pub fn has_pending_dialog(app: &tauri::AppHandle) -> bool {
    app.state::<AppState>()
        .pending_dialog
        .lock()
        .unwrap()
        .is_some()
}

pub fn resolve_pending_dialog_selection(
    app: &tauri::AppHandle,
    selected: &str,
) -> Result<String, String> {
    let selected_label = {
        let state = app.state::<AppState>();
        let pending = state.pending_dialog.lock().unwrap();
        let dialog = pending
            .as_ref()
            .ok_or_else(|| "No dialog is pending.".to_string())?;
        match_dialog_selection(selected, &dialog.options)
            .or_else(|| {
                dialog
                    .options
                    .iter()
                    .find(|option| option.label == selected)
                    .map(|option| option.label.clone())
            })
            .ok_or_else(|| "Selected option does not match the pending dialog.".to_string())?
    };

    let state = app.state::<AppState>();
    let dialog = state
        .pending_dialog
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| "No dialog is pending.".to_string())?;
    let _ = dialog.responder.send(selected_label.clone());
    let _ = app.emit("dismiss_dialog", ());
    Ok(selected_label)
}

fn handle_pending_dialog_answer(app: &tauri::AppHandle, transcript: &str) -> bool {
    let (is_pending, selected) = {
        let state = app.state::<AppState>();
        let pending = state.pending_dialog.lock().unwrap();
        if let Some(dialog) = pending.as_ref() {
            (true, match_dialog_selection(transcript, &dialog.options))
        } else {
            (false, None)
        }
    };

    if !is_pending {
        return false;
    }

    if let Some(selected) = selected {
        let _ = resolve_pending_dialog_selection(app, &selected);
    } else {
        let _ = emit_processing(
            app,
            ProcessingState {
                stage: "idle".to_string(),
                text: String::new(),
            },
        );
    }
    true
}

fn handle_command(app: &tauri::AppHandle, cmd: crate::command_parser::Command) {
    use crate::command_parser::Command;
    use crate::state::record_automation_event;

    match cmd {
        Command::SetMode(mode) => {
            let label = mode.to_string();
            let state = app.state::<AppState>();
            *state.app_mode.lock().unwrap() = mode;
            record_automation_event(app, "mode.changed", &label);
            let _ = app.emit("app_mode_changed", &label);
        }
        Command::Status => {
            let state = app.state::<AppState>();
            let mode = state.app_mode.lock().unwrap().to_string();
            record_automation_event(app, "command.status", &mode);
            let _ = app.emit("app_mode_changed", &mode);
        }
    }
}

fn handle_ui_command(app: &tauri::AppHandle, transcript: &str) -> bool {
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

    if contains_any(&text, &close_words) {
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

fn build_ui_context(app: &tauri::AppHandle) -> Option<String> {
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

fn forced_tool_for_transcript(transcript: &str, tools: &ToolsConfig) -> Option<String> {
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

fn match_dialog_selection(transcript: &str, options: &[DialogOption]) -> Option<String> {
    let text = normalize_selection_text(transcript);
    if text.is_empty() {
        return None;
    }

    let aliases: &[&[&str]] = &[
        &["a", "1", "eins", "erste", "erster", "erstes", "option a"],
        &[
            "b", "be", "bee", "2", "zwei", "zweite", "zweiter", "zweites", "option b",
        ],
        &[
            "c", "ce", "cee", "3", "drei", "dritte", "dritter", "drittes", "option c",
        ],
        &[
            "d", "de", "dee", "4", "vier", "vierte", "vierter", "viertes", "option d",
        ],
    ];

    let tokens = text.split_whitespace().collect::<Vec<_>>();
    for (idx, option) in options.iter().enumerate() {
        if idx < aliases.len()
            && aliases[idx].iter().any(|alias| {
                if alias.contains(' ') {
                    text == *alias || text.contains(alias)
                } else {
                    text == *alias || tokens.contains(alias)
                }
            })
        {
            return Some(option.label.clone());
        }
    }

    for option in options {
        let label = normalize_selection_text(&option.label);
        if label.is_empty() {
            continue;
        }
        if text == label || (label.len() >= 3 && (text.contains(&label) || label.contains(&text))) {
            return Some(option.label.clone());
        }
    }

    None
}

fn normalize_selection_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

async fn show_panel(app: &tauri::AppHandle, title: String, content: String) -> Result<(), String> {
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

fn clear_pending_dialog(app: &tauri::AppHandle, question: &str) {
    let state = app.state::<AppState>();
    let mut pending = state.pending_dialog.lock().unwrap();
    let should_clear = pending
        .as_ref()
        .map(|dialog| dialog.question == question)
        .unwrap_or(false);
    if should_clear {
        *pending = None;
    }
}

/// Execute a single tool call and return the result text.
async fn execute_tool(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    tool_call: &voice::OllamaToolCall,
) -> String {
    let rag_store = &app.state::<AppState>().rag_store;

    match tool_call.function.name.as_str() {
        "search_knowledge" => {
            let query = tool_call
                .function
                .arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let results = rag_store
                .search(&query, &config.llm_base_url, &config.embed_model, 5)
                .await
                .unwrap_or_default();

            if results.is_empty() {
                "No relevant results found in the knowledge base.".to_string()
            } else {
                results
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        format!(
                            "[{}] (source: {}, relevance: {:.2})\n{}",
                            i + 1,
                            r.source,
                            r.score,
                            r.text
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
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
                    match tools::describe_screenshot(&config.llm_base_url, &config.llm_provider, vision_model, &image_b64, &question).await {
                        Ok(desc) => desc,
                        Err(e) => format!("Screenshot captured but vision model failed: {}. The model '{}' may not support image inputs — try setting a vision model like 'llava' in settings.", e, vision_model),
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

const VOLATILE_TOOLS: &[&str] = &["get_current_time", "read_clipboard"];

fn redact_stale_tool_results(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let last_user_idx = messages.iter().rposition(|m| m.role == "user").unwrap_or(0);

    // Collect tool_call_ids from volatile tools in older turns.
    let mut volatile_ids = std::collections::HashSet::new();
    for msg in messages.iter().take(last_user_idx) {
        if let Some(ref calls) = msg.tool_calls {
            for call in calls {
                if VOLATILE_TOOLS.contains(&call.function.name.as_str()) {
                    if let Some(ref id) = call.id {
                        volatile_ids.insert(id.clone());
                    }
                }
            }
        }
    }

    messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            if i >= last_user_idx || volatile_ids.is_empty() {
                return msg.clone();
            }
            if msg.role == "tool" {
                if let Some(ref id) = msg.tool_call_id {
                    if volatile_ids.contains(id) {
                        return ChatMessage {
                            content: "(outdated)".to_string(),
                            ..msg.clone()
                        };
                    }
                }
                // XML-style tool results embedded in user messages won't have
                // tool_call_id — match by content pattern instead.
                if msg.content.contains("[Tool result for get_current_time]")
                    || msg.content.contains("[Tool result for read_clipboard]")
                {
                    return ChatMessage {
                        content: "(outdated)".to_string(),
                        ..msg.clone()
                    };
                }
            }
            // Also redact user messages that carry XML-style tool results.
            if msg.role == "user" && msg.content.starts_with("Here are the tool results") {
                let dominated_by_volatile = VOLATILE_TOOLS
                    .iter()
                    .any(|t| msg.content.contains(&format!("[Tool result for {}]", t)));
                if dominated_by_volatile {
                    return ChatMessage {
                        content: "(outdated tool results)".to_string(),
                        ..msg.clone()
                    };
                }
            }
            msg.clone()
        })
        .collect()
}
