use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio_util::sync::CancellationToken;

mod rag;
mod sandbox;
mod tools;
mod voice;

#[derive(Clone, Serialize)]
struct ProcessingState {
    stage: String,
    text: String,
}

#[derive(Clone, Serialize)]
struct AudioChunk {
    index: u32,
    audio: String, // base64 WAV
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    role: String,
    content: String,
    /// Preserved tool_calls from assistant messages.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    tool_calls: Option<Vec<voice::OllamaToolCallOut>>,
    /// OpenAI-compatible tool result messages must reference the call they answer.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    tool_call_id: Option<String>,
}

pub struct AppState {
    messages: Mutex<Vec<ChatMessage>>,
    config: Mutex<VoiceConfig>,
    rag_store: rag::RagStore,
    audit_log: Mutex<sandbox::AuditLog>,
    // Audio samples collected by the recording thread
    recorded_samples: Mutex<Vec<f32>>,
    recording_sample_rate: Mutex<u32>,
    is_recording: Mutex<bool>,
    // Cancellation token for the active pipeline — cancelled when user interrupts
    pipeline_cancel: Mutex<CancellationToken>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_true")]
    pub search_knowledge: bool,
    #[serde(default = "default_true")]
    pub screenshot: bool,
    #[serde(default = "default_true")]
    pub read_clipboard: bool,
    #[serde(default = "default_true")]
    pub open_url: bool,
    #[serde(default = "default_true")]
    pub get_current_time: bool,
    #[serde(default = "default_true")]
    pub list_apps: bool,
    #[serde(default)]
    pub run_command: bool,
    #[serde(default = "default_true")]
    pub web_fetch: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            search_knowledge: false,
            screenshot: false,
            read_clipboard: false,
            open_url: false,
            get_current_time: false,
            list_apps: false,
            run_command: false, // Off by default — powerful tool
            web_fetch: false,
        }
    }
}

fn default_llm_provider() -> String {
    "openai".to_string()
}

fn default_llm_base_url() -> String {
    "http://127.0.0.1:8081".to_string()
}

fn default_llm_model() -> String {
    "gemma".to_string()
}

fn default_whisper_server_url() -> String {
    "http://127.0.0.1:8350".to_string()
}

fn default_tts_url() -> String {
    "http://127.0.0.1:8005".to_string()
}

fn default_tts_voice() -> String {
    "de_DE-thorsten-medium".to_string()
}

pub fn core_system_prompt() -> &'static str {
    "You are a desktop voice assistant. Answer in the user's preferred language. Keep answers brief. Use available tools for current, local, or computer-specific information. If the user asks for date or time, use get_current_time. Do not claim you checked something unless you called the matching tool."
}

fn default_user_prompt() -> String {
    "Sprich grundsätzlich Deutsch, außer der Nutzer bittet ausdrücklich um eine andere Sprache. Halte Antworten kurz und gesprächig. Nutze keine Markdown-Formatierung, keine Codeblöcke und keine Aufzählungen.".to_string()
}

fn default_window_width() -> f64 { 380.0 }
fn default_window_height() -> f64 { 420.0 }

fn default_hotkey() -> String { "F9".to_string() }

#[derive(Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default)]
    pub decorations: bool,
    #[serde(default = "default_window_width")]
    pub width: f64,
    #[serde(default = "default_window_height")]
    pub height: f64,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            decorations: false,
            width: default_window_width(),
            height: default_window_height(),
            x: None,
            y: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default = "default_whisper_server_url")]
    pub whisper_server_url: String,
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,
    #[serde(default = "default_llm_base_url", alias = "ollama_url")]
    pub llm_base_url: String,
    #[serde(default = "default_llm_model", alias = "ollama_model")]
    pub llm_model: String,
    #[serde(default)]
    pub embed_model: String,
    #[serde(default)]
    pub vision_model: String,
    #[serde(default = "default_tts_url", alias = "chatterbox_url")]
    pub tts_url: String,
    #[serde(default = "default_tts_voice", alias = "chatterbox_voice")]
    pub tts_voice: String,
    #[serde(default)]
    pub debug_bubbles: bool,
    #[serde(default = "default_user_prompt")]
    pub system_prompt: String,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub sandbox: sandbox::SandboxConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            whisper_server_url: default_whisper_server_url(),
            llm_provider: default_llm_provider(),
            llm_base_url: default_llm_base_url(),
            llm_model: default_llm_model(),
            embed_model: String::new(),
            vision_model: String::new(),
            tts_url: default_tts_url(),
            tts_voice: default_tts_voice(),
            debug_bubbles: false,
            system_prompt: default_user_prompt(),
            tools: ToolsConfig::default(),
            sandbox: sandbox::SandboxConfig::default(),
            window: WindowConfig::default(),
            hotkey: default_hotkey(),
        }
    }
}

impl VoiceConfig {
    pub fn effective_system_prompt(&self) -> String {
        let user_prompt = self.system_prompt.trim();
        if user_prompt.is_empty() {
            core_system_prompt().to_string()
        } else {
            format!(
                "{}\n\nEditable user prompt:\n{}",
                core_system_prompt(),
                user_prompt
            )
        }
    }

    fn config_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("voice-assistant")
            .join("config.json")
    }

    fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str(&data) {
                    return config;
                }
            }
        }
        Self::default()
    }

    fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, data);
        }
    }
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> VoiceConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
#[allow(dead_code)]
fn get_core_system_prompt() -> String {
    core_system_prompt().to_string()
}

#[tauri::command]
fn set_config(app: tauri::AppHandle, state: tauri::State<AppState>, config: VoiceConfig) {
    config.save();
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_decorations(config.window.decorations);
    }

    let old_hotkey = { state.config.lock().unwrap().hotkey.clone() };
    if old_hotkey != config.hotkey {
        let _ = app.global_shortcut().unregister(old_hotkey.as_str());
        if let Err(e) = register_ptt_shortcut(&app, &config.hotkey) {
            eprintln!("Failed to register hotkey {:?}: {}", config.hotkey, e);
            // Fall back to old hotkey so the user isn't left without PTT.
            let _ = register_ptt_shortcut(&app, &old_hotkey);
        }
    }

    *state.config.lock().unwrap() = config;
    let _ = app.emit("config_changed", ());
}

#[tauri::command]
fn get_messages(state: tauri::State<AppState>) -> Vec<ChatMessage> {
    state.messages.lock().unwrap().clone()
}

#[tauri::command]
fn clear_messages(state: tauri::State<AppState>) {
    state.messages.lock().unwrap().clear();
}

#[tauri::command]
fn show_window(app: tauri::AppHandle) {
    reveal_main_window(&app, true);
}

fn handle_ptt_press(app: &tauri::AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut cancel = state.pipeline_cancel.lock().unwrap();
        cancel.cancel();
        *cancel = CancellationToken::new();
    }

    let _ = app.emit("pipeline_interrupted", ());
    reveal_main_window(app, false);
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

fn handle_ptt_release(app: &tauri::AppHandle) {
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
                    stage: "error".to_string(),
                    text: "No audio recorded".to_string(),
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

fn register_ptt_shortcut(
    app: &tauri::AppHandle,
    shortcut: &str,
) -> Result<(), tauri_plugin_global_shortcut::Error> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| match event.state {
            ShortcutState::Pressed => handle_ptt_press(app),
            ShortcutState::Released => handle_ptt_release(app),
        })
}

#[tauri::command]
fn hide_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn reveal_main_window(app: &tauri::AppHandle, reload: bool) {
    if let Some(window) = app.get_webview_window("main") {
        let state = app.state::<AppState>();
        let win_cfg = state.config.lock().unwrap().window.clone();

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

        if reload {
            let _ = window.eval("window.location.reload()");
        }
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

// ── RAG Commands ──

#[tauri::command]
async fn ingest_text(app: tauri::AppHandle, source: String, text: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    state
        .rag_store
        .ingest(&source, &text, &config.llm_base_url, &config.embed_model)
        .await
        .map_err(|e| format!("Ingest failed: {}", e))
}

#[tauri::command]
async fn ingest_file(app: tauri::AppHandle, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Read failed: {}", e))?;
    let source = std::path::Path::new(&path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let state = app.state::<AppState>();
    let config = state.config.lock().unwrap().clone();
    state
        .rag_store
        .ingest(&source, &text, &config.llm_base_url, &config.embed_model)
        .await
        .map_err(|e| format!("Ingest failed: {}", e))
}

#[tauri::command]
fn list_knowledge_sources(app: tauri::AppHandle) -> Result<Vec<(String, usize)>, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .list_sources()
        .map_err(|e| format!("List failed: {}", e))
}

#[tauri::command]
fn delete_knowledge_source(app: tauri::AppHandle, source: String) -> Result<usize, String> {
    let state = app.state::<AppState>();
    state
        .rag_store
        .delete_source(&source)
        .map_err(|e| format!("Delete failed: {}", e))
}

#[tauri::command]
fn start_recording(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Check if already recording
    {
        let is_rec = state.is_recording.lock().unwrap();
        if *is_rec {
            return Ok(());
        }
    }

    // Clear previous samples
    state.recorded_samples.lock().unwrap().clear();
    *state.is_recording.lock().unwrap() = true;

    let app_handle = app.clone();

    // Spawn recording on a dedicated thread (cpal::Stream isn't Send)
    std::thread::spawn(move || {
        if let Err(e) = voice::record_audio(&app_handle) {
            eprintln!("Recording error: {}", e);
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_recording_and_process(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    // Signal recording to stop
    *state.is_recording.lock().unwrap() = false;

    // Give a moment for the recording thread to finish writing samples
    std::thread::sleep(std::time::Duration::from_millis(100));

    let samples = state.recorded_samples.lock().unwrap().clone();
    let sample_rate = *state.recording_sample_rate.lock().unwrap();
    let config = state.config.lock().unwrap().clone();

    if samples.is_empty() {
        return Err("No audio recorded".to_string());
    }

    // Process in background
    let cancel_token = state.pipeline_cancel.lock().unwrap().clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = process_pipeline(
            app_handle.clone(),
            samples,
            sample_rate,
            config,
            cancel_token,
        )
        .await
        {
            if e != "interrupted" {
                eprintln!("Pipeline error: {}", e);
                let _ = app_handle.emit(
                    "processing",
                    ProcessingState {
                        stage: "error".to_string(),
                        text: e,
                    },
                );
            }
        }
    });

    Ok(())
}

async fn process_pipeline(
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
        return Err("No speech detected".to_string());
    }

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
                    r = voice::chat_streaming(&config, &all_msgs, &tools, forced_tool_this_round.as_deref(), &sentence_tx) => {
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
                    voice::StreamResult::ToolCalls(tool_calls, preamble, xml_parsed) => {
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
                                    "LLM response: tool_calls=[{}], preamble_chars={}, xml_parsed={}",
                                    names,
                                    preamble.chars().count(),
                                    xml_parsed
                                ),
                            );
                        }

                        if xml_parsed {
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
            let result = voice::chat_streaming(&config, &all_msgs, &[], None, &sentence_tx)
                .await
                .map_err(|e| format!("LLM failed: {}", e))?;

            match result {
                voice::StreamResult::Content(text) => Ok(text),
                voice::StreamResult::ToolCalls(_, _, _) => {
                    Err("Model returned tool calls after max rounds".to_string())
                }
            }
        })
    };

    // Drop our copy of sentence_tx so the channel closes when the spawned task finishes
    drop(sentence_tx);

    // Process sentences as they arrive from the stream → TTS → audio
    // Check cancellation between each TTS synthesis
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

        // Race TTS synthesis against cancellation
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
        llm_handle.abort(); // kill the LLM task
        return Err("interrupted".to_string());
    }

    let full_response = llm_handle
        .await
        .map_err(|e| format!("LLM task failed: {}", e))?
        .map_err(|e| e)?;

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
                    match tools::describe_screenshot(&config.llm_base_url, vision_model, &image_b64, &question).await {
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            messages: Mutex::new(Vec::new()),
            config: Mutex::new(VoiceConfig::load()),
            rag_store: rag::RagStore::new().expect("Failed to initialize RAG store"),
            audit_log: Mutex::new(sandbox::AuditLog::new()),
            recorded_samples: Mutex::new(Vec::new()),
            recording_sample_rate: Mutex::new(44100),
            is_recording: Mutex::new(false),
            pipeline_cancel: Mutex::new(CancellationToken::new()),
        })
        .setup(|app| {
            // Build tray menu
            let show_item = MenuItemBuilder::with_id("show", "Show/Hide Window").build(app)?;
            let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let clear_item = MenuItemBuilder::with_id("clear", "Clear Chat").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&show_item)
                .item(&settings_item)
                .item(&clear_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Build tray icon
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
                                reveal_main_window(app, true);
                            }
                        } else {
                            reveal_main_window(app, true);
                        }
                    }
                    "settings" => {
                        // If settings window already exists, just focus it
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        } else {
                            // Create a new settings window
                            let url = tauri::WebviewUrl::App("index.html?view=settings".into());
                            let _ = WebviewWindowBuilder::new(app, "settings", url)
                                .title("Voice Assistant — Settings")
                                .inner_size(720.0, 700.0)
                                .min_inner_size(600.0, 500.0)
                                .resizable(true)
                                .build();
                        }
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

            // Register global PTT shortcut from config so it works when window is hidden
            let initial_hotkey = app
                .state::<AppState>()
                .config
                .lock()
                .unwrap()
                .hotkey
                .clone();
            register_ptt_shortcut(app.handle(), &initial_hotkey)?;

// Hide dock icon on macOS
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Make webview background transparent and hide on launch
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
            get_config,
            get_core_system_prompt,
            set_config,
            get_messages,
            clear_messages,
            show_window,
            hide_window,
            ingest_text,
            ingest_file,
            list_knowledge_sources,
            delete_knowledge_source,
            start_recording,
            stop_recording_and_process,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
