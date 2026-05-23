//! Audio-→-STT-→-LLM-→-TTS-Pipeline plus PTT-Handler.

use crate::conversation::redact_stale_tool_results;
use crate::dialog_manager::handle_pending_dialog_answer;
use crate::panel_manager::{build_ui_context, handle_ui_command};
use crate::state::{
    emit_processing, update_processing_state, AudioChunk, ProcessingState,
};
use crate::tool_executor::{execute_tool, forced_tool_for_transcript};
use crate::window::reveal_main_window;
use crate::{voice, AppState, ChatMessage, VoiceConfig};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

pub use crate::dialog_manager::{has_pending_dialog, resolve_pending_dialog_selection};

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
            return run_agent_session(&app, &mode, &transcript, &config).await;
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

    // LLM with tool calling → streaming TTS
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

    drop(sentence_tx);

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

async fn run_agent_session(
    app: &tauri::AppHandle,
    mode: &crate::state::AppMode,
    prompt: &str,
    _config: &VoiceConfig,
) -> Result<(), String> {
    use crate::agent_session;
    use crate::state::record_automation_event;

    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let session_name = agent_session::ensure_session(mode, &working_dir).await?;

    agent_session::open_terminal(&session_name).await?;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    agent_session::send_keys(&session_name, prompt).await?;

    record_automation_event(
        app,
        "agent.sent",
        &format!("{}:{}", mode, &prompt[..prompt.len().min(80)]),
    );

    let _ = app.emit(
        "assistant_text",
        &format!("➜ {} — Eingabe gesendet", mode),
    );

    emit_processing(
        app,
        ProcessingState {
            stage: "idle".to_string(),
            text: String::new(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}

fn handle_command(app: &tauri::AppHandle, cmd: crate::command_parser::Command) {
    use crate::command_parser::Command;
    use crate::state::record_automation_event;

    match cmd {
        Command::SetMode(mode) => {
            let label = mode.to_string();
            let state = app.state::<AppState>();
            *state.app_mode.lock().unwrap() = mode.clone();
            record_automation_event(app, "mode.changed", &label);
            let _ = app.emit("app_mode_changed", &label);

            if mode != crate::state::AppMode::Chat {
                tauri::async_runtime::spawn(async move {
                    let working_dir =
                        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                    match crate::agent_session::ensure_session(&mode, &working_dir).await {
                        Ok(name) => {
                            let _ = crate::agent_session::open_terminal(&name).await;
                        }
                        Err(e) => {
                            eprintln!("Agent-Session konnte nicht gestartet werden: {}", e);
                        }
                    }
                });
            }
        }
        Command::Status => {
            let state = app.state::<AppState>();
            let mode = state.app_mode.lock().unwrap().to_string();
            record_automation_event(app, "command.status", &mode);
            let _ = app.emit("app_mode_changed", &mode);
        }
    }
}
