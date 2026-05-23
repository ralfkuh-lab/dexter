//! Audio-→-STT-→-LLM-→-TTS-Pipeline plus PTT-Handler und Tool-Ausführung.

use crate::state::{AudioChunk, ProcessingState};
use crate::window::reveal_main_window;
use crate::{sandbox, tools, voice, AppState, ChatMessage, ToolsConfig, VoiceConfig};
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

pub fn handle_ptt_press(app: &tauri::AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut cancel = state.pipeline_cancel.lock().unwrap();
        cancel.cancel();
        *cancel = CancellationToken::new();
    }

    let _ = app.emit("pipeline_interrupted", ());
    reveal_main_window(app);
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
            let _ = app_clone.emit(
                "processing",
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
                    let _ = app_clone.emit(
                        "processing",
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
    app.emit(
        "processing",
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
        app.emit(
            "processing",
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
    app.emit(
        "processing",
        ProcessingState {
            stage: "transcribed".to_string(),
            text: transcript.clone(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

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
    app.emit(
        "processing",
        ProcessingState {
            stage: "thinking".to_string(),
            text: "Thinking...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    let all_messages = app.state::<AppState>().messages.lock().unwrap().clone();

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
                                format!("LLM response: content only, {} chars", text.chars().count()),
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

                                let _ = app.emit(
                                    "processing",
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

                                let _ = app.emit(
                                    "processing",
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

                        let _ = app.emit(
                            "processing",
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
                let _ = app.emit("llm_debug", "LLM final request: tools disabled after max rounds");
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

        app.emit(
            "processing",
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

    Ok(())
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

            let _ = app.emit(
                "processing",
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
                let _ = app.emit(
                    "processing",
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
