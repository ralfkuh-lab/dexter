//! Audio-→-STT-→-LLM-→-TTS-Pipeline plus PTT-Handler.

use crate::conversation::redact_stale_tool_results;
use crate::dialog_manager::handle_pending_dialog_answer;
use crate::panel_manager::{build_ui_context, handle_ui_command};
use crate::state::{emit_processing, update_processing_state, AudioChunk, ProcessingState};
use crate::tool_executor::{execute_tool, forced_tool_for_transcript};
use crate::window::reveal_main_window;
use crate::{voice, AppState, ChatMessage, VoiceConfig};
use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize)]
struct DebugEvent {
    summary: String,
    detail: String,
}

pub use crate::dialog_manager::{has_pending_dialog, resolve_pending_dialog_selection};

pub fn handle_ptt_press(app: &tauri::AppHandle) {
    if crate::dictation::is_active(app) || crate::hands_free::is_active(app) {
        return;
    }

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
    if crate::dictation::is_active(app) || crate::hands_free::is_active(app) {
        return;
    }

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

pub fn start_dictation_loop(app: &tauri::AppHandle) {
    let cancel_token = {
        let state = app.state::<AppState>();
        let mut active = state.dictation_cancel.lock().unwrap();
        if active.as_ref().is_some_and(|token| !token.is_cancelled()) {
            return;
        }
        let token = CancellationToken::new();
        *active = Some(token.clone());
        token
    };

    let (segment_tx, segment_rx) = std::sync::mpsc::channel::<voice::AudioSegment>();
    let app_for_recording = app.clone();
    let recording_cancel = cancel_token.clone();
    std::thread::spawn(move || {
        if let Err(e) = voice::record_continuous(&app_for_recording, segment_tx, recording_cancel) {
            eprintln!("Continuous recording error: {}", e);
        }
    });

    let (async_tx, mut async_rx) = tokio::sync::mpsc::channel::<voice::AudioSegment>(8);
    let bridge_cancel = cancel_token.clone();
    std::thread::spawn(move || {
        while !bridge_cancel.is_cancelled() {
            match segment_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(segment) => {
                    if async_tx.blocking_send(segment).is_err() {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let app_for_loop = app.clone();
    let loop_cancel = cancel_token.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(segment) = async_rx.recv().await {
            if loop_cancel.is_cancelled() || !crate::dictation::is_active(&app_for_loop) {
                break;
            }

            let config = {
                app_for_loop
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .clone()
            };

            if let Err(e) = process_dictation_segment(
                &app_for_loop,
                segment,
                &config.whisper_server_url,
                &loop_cancel,
            )
            .await
            {
                if e != "interrupted" {
                    eprintln!("Dictation segment error: {}", e);
                    let _ = emit_processing(
                        &app_for_loop,
                        ProcessingState {
                            stage: "error".to_string(),
                            text: e,
                        },
                    );
                }
            }
        }
    });
}

pub fn stop_dictation_loop(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let token = { state.dictation_cancel.lock().unwrap().take() };
    if let Some(token) = token {
        token.cancel();
    }
}

pub fn start_hands_free_loop(app: &tauri::AppHandle) {
    let cancel_token = {
        let state = app.state::<AppState>();
        let mut active = state.hands_free_cancel.lock().unwrap();
        if active.as_ref().is_some_and(|token| !token.is_cancelled()) {
            return;
        }
        let token = CancellationToken::new();
        *active = Some(token.clone());
        token
    };

    let (segment_tx, segment_rx) = std::sync::mpsc::channel::<voice::AudioSegment>();
    let app_for_recording = app.clone();
    let recording_cancel = cancel_token.clone();
    std::thread::spawn(move || {
        if let Err(e) = voice::record_continuous(&app_for_recording, segment_tx, recording_cancel) {
            eprintln!("Hands-free recording error: {}", e);
        }
    });

    let (async_tx, mut async_rx) = tokio::sync::mpsc::channel::<voice::AudioSegment>(4);
    let bridge_cancel = cancel_token.clone();
    std::thread::spawn(move || {
        while !bridge_cancel.is_cancelled() {
            match segment_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(segment) => {
                    if async_tx.blocking_send(segment).is_err() {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let app_for_loop = app.clone();
    let loop_cancel = cancel_token.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(segment) = async_rx.recv().await {
            if loop_cancel.is_cancelled() || !crate::hands_free::is_active(&app_for_loop) {
                break;
            }

            let config = {
                app_for_loop
                    .state::<AppState>()
                    .config
                    .lock()
                    .unwrap()
                    .clone()
            };

            if let Err(e) =
                process_hands_free_segment(&app_for_loop, segment, config, &loop_cancel).await
            {
                if e != "interrupted" {
                    eprintln!("Hands-free segment error: {}", e);
                    let _ = emit_processing(
                        &app_for_loop,
                        ProcessingState {
                            stage: "error".to_string(),
                            text: e,
                        },
                    );
                }
            }
        }
    });
}

pub fn stop_hands_free_loop(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let token = { state.hands_free_cancel.lock().unwrap().take() };
    if let Some(token) = token {
        token.cancel();
    }
}

async fn process_dictation_segment(
    app: &tauri::AppHandle,
    segment: voice::AudioSegment,
    whisper_server_url: &str,
    cancel: &CancellationToken,
) -> Result<(), String> {
    emit_processing(
        app,
        ProcessingState {
            stage: "transcribing".to_string(),
            text: "Diktat wird transkribiert...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let transcript =
        voice::transcribe_audio_http(whisper_server_url, &segment.samples, segment.sample_rate)
            .await
            .map_err(|e| format!("Transcription failed: {}", e))?;

    if cancel.is_cancelled() || !crate::dictation::is_active(app) {
        return Err("interrupted".to_string());
    }

    if transcript.trim().is_empty() {
        emit_processing(
            app,
            ProcessingState {
                stage: "listening".to_string(),
                text: "Höre zu...".to_string(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
        return Ok(());
    }

    let should_send = crate::dictation::append_segment(app, &transcript);
    if should_send {
        crate::dictation::send_buffer(app).await?;
    }

    if crate::dictation::is_active(app) && !cancel.is_cancelled() {
        emit_processing(
            app,
            ProcessingState {
                stage: "listening".to_string(),
                text: "Höre zu...".to_string(),
            },
        )
        .map_err(|e: tauri::Error| e.to_string())?;
    }

    Ok(())
}

async fn process_hands_free_segment(
    app: &tauri::AppHandle,
    segment: voice::AudioSegment,
    config: VoiceConfig,
    loop_cancel: &CancellationToken,
) -> Result<(), String> {
    emit_processing(
        app,
        ProcessingState {
            stage: "transcribing".to_string(),
            text: "Transkribiere...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let transcript = voice::transcribe_audio_http(
        &config.whisper_server_url,
        &segment.samples,
        segment.sample_rate,
    )
    .await
    .map_err(|e| format!("Transcription failed: {}", e))?;

    if loop_cancel.is_cancelled() || !crate::hands_free::is_active(app) {
        return Err("interrupted".to_string());
    }

    if transcript.trim().is_empty() {
        emit_hands_free_listening(app)?;
        return Ok(());
    }

    if should_ignore_hands_free_transcript(&transcript) {
        emit_hands_free_listening(app)?;
        return Ok(());
    }

    if is_agent_mode(app) {
        crate::agent_draft::process_segment(app, &transcript, &config).await?;

        if crate::hands_free::is_active(app) && !loop_cancel.is_cancelled() {
            emit_hands_free_listening(app)?;
        }

        return Ok(());
    }

    let turn_cancel = if has_pending_dialog(app) {
        app.state::<AppState>()
            .pipeline_cancel
            .lock()
            .unwrap()
            .clone()
    } else {
        let token = crate::hands_free::pipeline_cancel_token(app);
        let _ = app.emit("pipeline_interrupted", ());
        token
    };

    run_llm_pipeline(app.clone(), transcript, config, turn_cancel).await?;

    if crate::hands_free::is_active(app) && !loop_cancel.is_cancelled() {
        emit_hands_free_listening(app)?;
    }

    Ok(())
}

fn emit_hands_free_listening(app: &tauri::AppHandle) -> Result<(), String> {
    emit_processing(
        app,
        ProcessingState {
            stage: "listening".to_string(),
            text: "Höre zu...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())
}

fn is_agent_mode(app: &tauri::AppHandle) -> bool {
    *app.state::<AppState>().app_mode.lock().unwrap() != crate::state::AppMode::Chat
}

fn should_ignore_hands_free_transcript(transcript: &str) -> bool {
    let normalized = transcript
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || matches!(c, '…' | '„' | '“' | '”'))
        .to_lowercase();

    if normalized.is_empty() {
        return true;
    }

    matches!(
        normalized.as_str(),
        "hm" | "hmm"
            | "hmmm"
            | "mmm"
            | "mhm"
            | "mh"
            | "äh"
            | "ähm"
            | "eh"
            | "ehm"
            | "uh"
            | "uhm"
            | "um"
    )
}

fn is_agent_enter_command(transcript: &str) -> bool {
    let normalized = transcript
        .trim()
        .trim_matches(|c: char| c.is_ascii_punctuation() || matches!(c, '…' | '„' | '“' | '”'))
        .to_lowercase();

    matches!(normalized.as_str(), "enter" | "return" | "eingabe")
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

    if crate::dictation::is_active(&app) {
        let should_send = crate::dictation::append_segment(&app, &transcript);
        if should_send {
            crate::dictation::send_buffer(&app).await?;
        } else {
            emit_processing(
                &app,
                ProcessingState {
                    stage: "idle".to_string(),
                    text: String::new(),
                },
            )
            .map_err(|e: tauri::Error| e.to_string())?;
        }
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
            if is_agent_enter_command(&transcript) {
                return run_agent_enter(&app, &mode).await;
            }
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
                    let msgs_json = serde_json::to_string_pretty(&all_msgs).unwrap_or_default();
                    let _ = app.emit(
                        "llm_debug",
                        DebugEvent {
                            summary: format!(
                                "→ LLM {} · {} msgs · {} tools{}",
                                config.llm_model,
                                all_msgs.len(),
                                tools.len(),
                                forced_tool_next.as_deref().map(|t| format!(" · forced={}", t)).unwrap_or_default()
                            ),
                            detail: format!("## LLM Request\n\n**Provider:** {}\n**Model:** {}\n**Messages:** {}\n**Tools:** {}\n**Forced tool:** {}\n\n### Messages\n\n```json\n{}\n```",
                                config.llm_provider,
                                config.llm_model,
                                all_msgs.len(),
                                tools.len(),
                                forced_tool_next.as_deref().unwrap_or("none"),
                                msgs_json
                            ),
                        },
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
                                DebugEvent {
                                    summary: format!("← content · {} chars", text.chars().count()),
                                    detail: format!("## LLM Response (Content)\n\n{}", text),
                                },
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
                            let args_summary: Vec<String> = tool_calls
                                .iter()
                                .map(|tc| {
                                    let args_str = serde_json::to_string(&tc.function.arguments)
                                        .unwrap_or_default();
                                    if tc.function.name == "run_command" {
                                        let cmd = tc
                                            .function
                                            .arguments
                                            .get("command")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("?");
                                        format!("run_command: `{}`", cmd)
                                    } else {
                                        let short = if args_str.len() > 60 {
                                            format!("{}…", &args_str[..57])
                                        } else {
                                            args_str.clone()
                                        };
                                        format!("{}: {}", tc.function.name, short)
                                    }
                                })
                                .collect();
                            let calls_json = serde_json::to_string_pretty(
                                &tool_calls
                                    .iter()
                                    .map(|tc| {
                                        serde_json::json!({
                                            "name": tc.function.name,
                                            "arguments": tc.function.arguments,
                                            "id": tc.id,
                                        })
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .unwrap_or_default();
                            let _ = app.emit(
                                "llm_debug",
                                DebugEvent {
                                    summary: format!("← {}", args_summary.join(" · ")),
                                    detail: format!("## Tool Calls\n\n**Source:** {:?}\n**Preamble:** {} chars\n\n```json\n{}\n```{}",
                                        source,
                                        preamble.chars().count(),
                                        calls_json,
                                        if preamble.is_empty() { String::new() } else { format!("\n\n### Preamble\n\n{}", preamble) }
                                    ),
                                },
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
                let msgs_json = serde_json::to_string_pretty(&all_msgs).unwrap_or_default();
                let _ = app.emit(
                    "llm_debug",
                    DebugEvent {
                        summary: format!("→ LLM {} · {} msgs · no tools (max rounds)", config.llm_model, all_msgs.len()),
                        detail: format!("## LLM Final Request (no tools)\n\n**Messages:** {}\n\n```json\n{}\n```", all_msgs.len(), msgs_json),
                    },
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

    let session = agent_session::ensure_session(mode, &working_dir).await?;

    agent_session::open_terminal(&session.name).await?;
    if session.created {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    agent_session::send_keys(&session.pane_id, prompt).await?;

    record_automation_event(
        app,
        "agent.sent",
        &format!("{}:{}", mode, &prompt[..prompt.len().min(80)]),
    );

    let _ = app.emit("assistant_text", &format!("➜ {} — Eingabe gesendet", mode));

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

async fn run_agent_enter(
    app: &tauri::AppHandle,
    mode: &crate::state::AppMode,
) -> Result<(), String> {
    use crate::agent_session;
    use crate::state::record_automation_event;

    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let session = agent_session::ensure_session(mode, &working_dir).await?;

    agent_session::open_terminal(&session.name).await?;
    agent_session::send_enter(&session.pane_id).await?;

    record_automation_event(app, "agent.enter", mode.to_string());

    let _ = app.emit("assistant_text", &format!("↵ {} — Enter gesendet", mode));

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
                        Ok(session) => {
                            let _ = crate::agent_session::open_terminal(&session.name).await;
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
        Command::ToggleDictation => {
            if crate::dictation::is_active(app) {
                crate::dictation::deactivate(app);
                record_automation_event(app, "dictation", "deactivated");
            } else {
                crate::dictation::activate(app);
                record_automation_event(app, "dictation", "activated");
            }
        }
        Command::ToggleHandsFree => {
            if crate::hands_free::is_active(app) {
                crate::hands_free::deactivate(app);
                record_automation_event(app, "hands_free", "deactivated");
            } else {
                crate::hands_free::activate(app);
                record_automation_event(app, "hands_free", "activated");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_agent_enter_command, should_ignore_hands_free_transcript};

    #[test]
    fn ignores_hands_free_fillers() {
        for text in ["Mmm", "ähm", "Hm.", "…mhm…"] {
            assert!(
                should_ignore_hands_free_transcript(text),
                "{text:?} should be ignored"
            );
        }
    }

    #[test]
    fn keeps_short_meaningful_hands_free_inputs() {
        for text in ["ja", "nein", "A", "OK", "teste bitte mal", "drück enter"] {
            assert!(
                !should_ignore_hands_free_transcript(text),
                "{text:?} should be kept"
            );
        }
    }

    #[test]
    fn recognizes_agent_enter_commands() {
        for text in ["Enter", "return.", "Eingabe"] {
            assert!(
                is_agent_enter_command(text),
                "{text:?} should be treated as Enter"
            );
        }
        assert!(!is_agent_enter_command("drück enter"));
    }
}
