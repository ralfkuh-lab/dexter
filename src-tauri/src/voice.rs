use crate::{AppState, ChatMessage, VoiceConfig};
use base64::{engine::general_purpose::STANDARD, Engine};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Manager;
use tokio::sync::mpsc;

// ── Audio Recording (cpal) ──

/// Record audio on the current thread until `is_recording` is set to false.
/// Writes samples directly into AppState's shared buffer.
pub fn record_audio(
    app: &tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("No input device available")?;

    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    // Store sample rate in state
    {
        let state = app.state::<AppState>();
        *state.recording_sample_rate.lock().unwrap() = sample_rate;
    }

    let app_clone = app.clone();

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let app_ref = app.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let state = app_ref.state::<AppState>();
                    if channels <= 1 {
                        state
                            .recorded_samples
                            .lock()
                            .unwrap()
                            .extend_from_slice(data);
                    } else {
                        let mono: Vec<f32> = data
                            .chunks(channels)
                            .map(|frame| frame.iter().copied().sum::<f32>() / frame.len() as f32)
                            .collect();
                        state
                            .recorded_samples
                            .lock()
                            .unwrap()
                            .extend_from_slice(&mono);
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        cpal::SampleFormat::I16 => {
            let app_ref = app.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = if channels <= 1 {
                        data.iter().map(|&s| s as f32 / 32768.0).collect()
                    } else {
                        data.chunks(channels)
                            .map(|frame| {
                                frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                                    / frame.len() as f32
                            })
                            .collect()
                    };
                    let state = app_ref.state::<AppState>();
                    state
                        .recorded_samples
                        .lock()
                        .unwrap()
                        .extend_from_slice(&floats);
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )?
        }
        format => {
            return Err(format!("Unsupported sample format: {:?}", format).into());
        }
    };

    stream.play()?;

    // Spin until recording is stopped
    loop {
        let is_rec = *app_clone.state::<AppState>().is_recording.lock().unwrap();
        if !is_rec {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // Stream drops here, stopping recording
    Ok(())
}

pub async fn transcribe_audio_http(
    base_url: &str,
    samples: &[f32],
    source_sample_rate: u32,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let audio_16k = if source_sample_rate != 16000 {
        resample(samples, source_sample_rate, 16000)
    } else {
        samples.to_vec()
    };

    let mut body = Vec::with_capacity(audio_16k.len() * std::mem::size_of::<f32>());
    for sample in audio_16k {
        body.extend_from_slice(&sample.to_le_bytes());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let resp = client
        .post(format!("{}/transcribe", trim_base_url(base_url)))
        .body(body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Whisper HTTP error {}: {}", status, body).into());
    }

    let json: serde_json::Value = resp.json().await?;
    if let Some(error) = json.get("error").and_then(|v| v.as_str()) {
        return Err(format!("Whisper HTTP error: {}", error).into());
    }

    Ok(json
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}

fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    let ratio = to_rate as f64 / from_rate as f64;
    let output_len = (input.len() as f64 * ratio) as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_idx = i as f64 / ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < input.len() {
            input[idx] as f64 * (1.0 - frac) + input[idx + 1] as f64 * frac
        } else if idx < input.len() {
            input[idx] as f64
        } else {
            0.0
        };

        output.push(sample as f32);
    }

    output
}

// ── LLM Chat ──

#[derive(Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCallOut>>,
}

/// Serializable tool call (for sending back to Ollama in assistant messages).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OllamaToolCallOut {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    pub function: OllamaToolFunctionOut,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OllamaToolFunctionOut {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
pub struct OllamaResponseMessage {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OllamaToolCall {
    #[serde(default)]
    pub id: Option<String>,
    pub function: OllamaToolFunction,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OllamaToolFunction {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

impl OllamaToolCall {
    /// Convert deserialized tool call to serializable form for echoing back.
    pub fn to_out(&self) -> OllamaToolCallOut {
        OllamaToolCallOut {
            id: self.id.clone(),
            function: OllamaToolFunctionOut {
                name: self.function.name.clone(),
                arguments: self.function.arguments.clone(),
            },
        }
    }
}

/// Streaming chat response from Ollama, split into sentences.
#[derive(Deserialize)]
struct OllamaStreamChunk {
    message: Option<OllamaResponseMessage>,
    done: Option<bool>,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCallOut>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    stream: bool,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_template_kwargs: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct OpenAiToolCallOut {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiToolFunctionOut,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct OpenAiToolFunctionOut {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiDelta,
}

#[derive(Deserialize)]
struct OpenAiDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Deserialize)]
struct OpenAiToolCallDelta {
    index: Option<usize>,
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<OpenAiToolFunctionDelta>,
    // llama.cpp has emitted this flatter shape in some tool-call handlers.
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiToolFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Default)]
struct OpenAiToolCallAccumulator {
    id: Option<String>,
    kind: Option<String>,
    name: String,
    arguments: String,
}

/// Build tool definitions based on enabled tools in config.
pub fn build_tools(tools_config: &crate::ToolsConfig) -> Vec<serde_json::Value> {
    let shell_name = if cfg!(target_os = "macos") {
        "zsh"
    } else if cfg!(target_os = "windows") {
        "PowerShell"
    } else {
        "sh"
    };

    let mut tools = Vec::new();

    if tools_config.search_knowledge {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "search_knowledge",
                "description": "Search the user's local knowledge base for relevant information. Use this when the user asks about something that might be in their stored documents or notes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query to find relevant knowledge"
                        }
                    },
                    "required": ["query"]
                }
            }
        }));
    }

    if tools_config.screenshot {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "take_screenshot",
                "description": "Capture a screenshot of the user's screen and describe what is visible. Use this when the user asks what's on their screen, asks you to look at something, or wants help with something they're looking at. By default captures the active monitor (where the mouse cursor is).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": {
                            "type": "string",
                            "description": "What to look for or describe in the screenshot. Defaults to a general description."
                        },
                        "monitor": {
                            "type": "integer",
                            "description": "Which monitor to capture (1 = primary, 2 = secondary, etc). If omitted, captures the active monitor where the mouse cursor is."
                        }
                    }
                }
            }
        }));
    }

    if tools_config.read_clipboard {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_clipboard",
                "description": "Read the current text contents of the user's clipboard. Use this when the user says they copied something, or asks about what's in their clipboard. The clipboard changes constantly — ALWAYS call this fresh every time it is referenced; never reuse a previous result from earlier in the conversation, even if you just called it moments ago.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.open_url {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "open_url",
                "description": "Open a URL in the user's default web browser. Use when the user asks to open a website, search something on the web, or navigate to a URL.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to open"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    if tools_config.get_current_time {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Get the current date, time, and day of week. Use when the user asks what time or date it is. Time advances continuously — ALWAYS call this fresh every time the user asks; never reuse a previous result, even if you just answered a time question seconds ago.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.list_apps {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_running_apps",
                "description": "List the user's currently running applications or open windows. Use when the user asks what apps are open or running.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }));
    }

    if tools_config.web_fetch {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Fetch a web page and return its text content. Use when the user asks about something online, wants you to read an article, check a website, look up documentation, or get current information from the web.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch"
                        }
                    },
                    "required": ["url"]
                }
            }
        }));
    }

    if tools_config.run_command {
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "run_command",
                "description": format!("Execute a shell command on the user's computer and return its output. Use when the user asks to check system status, manage files, run scripts, install something, or perform any task that requires terminal access. Always prefer specific, minimal commands. The command runs in {}.", shell_name),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": format!("The shell command to execute (runs in {})", shell_name)
                        }
                    },
                    "required": ["command"]
                }
            }
        }));
    }

    tools
}

/// Result of a streaming chat — either the model streamed content (sentences sent
/// via channel) or it requested tool calls.
pub enum StreamResult {
    /// Model streamed a text response. Full text returned here.
    Content(String),
    /// Model requested tool calls. May include pre-tool-call narration text
    /// (e.g. "Let me search for that") that was already sent to TTS.
    /// Fields: (tool_calls, spoken_preamble, xml_parsed)
    /// xml_parsed=true means the model emitted XML text, not native Ollama tool_calls.
    ToolCalls(Vec<OllamaToolCall>, String, bool),
}

/// Unified streaming chat. Streams with tools enabled.
/// - If the model produces content tokens → sentences are sent via `sentence_tx`, returns `StreamResult::Content`.
/// - If the model returns tool_calls → returns `StreamResult::ToolCalls` (nothing sent via channel).
/// - Also handles XML-style tool calls from models that don't use native format.
pub async fn chat_streaming(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    forced_tool: Option<&str>,
    sentence_tx: &mpsc::Sender<String>,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    if config.llm_provider == "ollama" {
        return chat_streaming_ollama(config, messages, tools, sentence_tx).await;
    }

    chat_streaming_openai(config, messages, tools, forced_tool, sentence_tx).await
}

async fn chat_streaming_ollama(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    sentence_tx: &mpsc::Sender<String>,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let mut ollama_messages = vec![OllamaMessage {
        role: "system".to_string(),
        content: config.effective_system_prompt(),
        tool_calls: None,
    }];

    for msg in messages {
        ollama_messages.push(OllamaMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_calls: msg.tool_calls.clone(),
        });
    }

    let request = OllamaChatRequest {
        model: config.llm_model.clone(),
        messages: ollama_messages,
        stream: true,
        tools: if tools.is_empty() {
            None
        } else {
            Some(tools.to_vec())
        },
    };

    let resp = client
        .post(format!("{}/api/chat", trim_base_url(&config.llm_base_url)))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama API error {}: {}", status, body).into());
    }

    use tokio_stream::StreamExt;

    let mut full_response = String::new();
    let mut sentence_buffer = String::new();
    let mut spoken_text = String::new(); // Text that was sent to TTS before a tool call
    let mut byte_stream = resp.bytes_stream();
    let mut line_buffer = Vec::new();
    let mut collected_tool_calls: Vec<OllamaToolCall> = Vec::new();
    // XML tool call collection state — only active when tools are provided
    let has_tools = !tools.is_empty();
    let mut xml_collecting = false;
    let mut xml_buffer = String::new();

    // Regex to detect XML tool call open tags like <tool_call> or <minimax:tool_call>
    let xml_open_re = regex::Regex::new(r"<(?:\w+:)?tool_call>").unwrap();
    let xml_close_re = regex::Regex::new(r"</(?:\w+:)?tool_call>").unwrap();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        line_buffer.extend_from_slice(&chunk);

        while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
            let line_str = String::from_utf8_lossy(&line);
            let line_str = line_str.trim();

            if line_str.is_empty() {
                continue;
            }

            if let Ok(stream_chunk) = serde_json::from_str::<OllamaStreamChunk>(line_str) {
                if let Some(msg) = &stream_chunk.message {
                    // Collect native tool calls if present
                    if let Some(tc) = &msg.tool_calls {
                        collected_tool_calls.extend(tc.clone());
                    }

                    // Collect content tokens
                    if !msg.content.is_empty() {
                        full_response.push_str(&msg.content);

                        if xml_collecting {
                            // We're inside an XML tool call block — collect into xml_buffer
                            xml_buffer.push_str(&msg.content);

                            // Check if the closing tag has arrived
                            if xml_close_re.is_match(&xml_buffer) {
                                // Parse the complete XML tool call block
                                let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                    collected_tool_calls.extend(parsed);
                                }
                                xml_buffer.clear();
                                xml_collecting = false;
                            }
                        } else {
                            sentence_buffer.push_str(&msg.content);

                            // Check if an XML tool call tag appeared in the sentence buffer
                            // Only detect XML tool calls when tools are provided
                            if has_tools && xml_open_re.find(&sentence_buffer).is_some() {
                                let m = xml_open_re.find(&sentence_buffer).unwrap();
                                // Flush everything before the tag to TTS
                                let before = sentence_buffer[..m.start()].trim().to_string();
                                if !before.is_empty() {
                                    spoken_text.push_str(&before);
                                    spoken_text.push(' ');
                                    let _ = sentence_tx.send(before).await;
                                }
                                // Everything after the open tag goes into xml_buffer
                                let after_tag = &sentence_buffer[m.end()..];
                                xml_buffer = after_tag.to_string();
                                sentence_buffer.clear();
                                xml_collecting = true;

                                // Check if closing tag is already in the buffer
                                if xml_close_re.is_match(&xml_buffer) {
                                    let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                    if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                        collected_tool_calls.extend(parsed);
                                    }
                                    xml_buffer.clear();
                                    xml_collecting = false;
                                }
                            } else {
                                // Normal streaming — send complete sentences as they form
                                while let Some(split_pos) = find_sentence_end(&sentence_buffer) {
                                    let sentence: String =
                                        sentence_buffer.drain(..=split_pos).collect();
                                    let sentence = sentence.trim().to_string();
                                    if !sentence.is_empty() {
                                        spoken_text.push_str(&sentence);
                                        spoken_text.push(' ');
                                        let _ = sentence_tx.send(sentence).await;
                                    }
                                }
                            }
                        }
                    }
                }

                if stream_chunk.done.unwrap_or(false) {
                    // Flush remaining sentence buffer (only if not collecting XML)
                    if !xml_collecting {
                        let remaining = sentence_buffer.trim().to_string();
                        if !remaining.is_empty() {
                            spoken_text.push_str(&remaining);
                            let _ = sentence_tx.send(remaining).await;
                        }
                    }
                    break;
                }
            }
        }
    }

    // Handle trailing data without newline
    if !line_buffer.is_empty() {
        let line_str = String::from_utf8_lossy(&line_buffer);
        if let Ok(stream_chunk) = serde_json::from_str::<OllamaStreamChunk>(line_str.trim()) {
            if let Some(msg) = &stream_chunk.message {
                if let Some(tc) = &msg.tool_calls {
                    collected_tool_calls.extend(tc.clone());
                }
                if !msg.content.is_empty() {
                    full_response.push_str(&msg.content);
                    if xml_collecting {
                        xml_buffer.push_str(&msg.content);
                    } else {
                        sentence_buffer.push_str(&msg.content);
                    }
                }
            }
        }
        // Try to parse any remaining XML
        if xml_collecting && xml_close_re.is_match(&xml_buffer) {
            let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
            if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                collected_tool_calls.extend(parsed);
            }
        }
        if !xml_collecting {
            let remaining = sentence_buffer.trim().to_string();
            if !remaining.is_empty() {
                spoken_text.push_str(&remaining);
                let _ = sentence_tx.send(remaining).await;
            }
        }
    }

    // If we got tool calls (native or XML-parsed), return them with any spoken preamble
    if !collected_tool_calls.is_empty() {
        return Ok(StreamResult::ToolCalls(
            collected_tool_calls,
            spoken_text.trim().to_string(),
            false,
        ));
    }

    // Last-resort fallback: check full response for XML tool calls we might have missed
    // Only when tools are provided — otherwise model's XML output is just text
    if has_tools && !full_response.is_empty() {
        if let Some(parsed) = parse_xml_tool_calls(&full_response) {
            if !parsed.is_empty() {
                return Ok(StreamResult::ToolCalls(
                    parsed,
                    spoken_text.trim().to_string(),
                    true,
                ));
            }
        }
    }

    Ok(StreamResult::Content(full_response.trim().to_string()))
}

async fn chat_streaming_openai(
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    forced_tool: Option<&str>,
    sentence_tx: &mpsc::Sender<String>,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let mut openai_messages = vec![OpenAiMessage {
        role: "system".to_string(),
        content: Some(crate::core_system_prompt().to_string()),
        tool_calls: None,
        tool_call_id: None,
    }];

    let user_prompt = config.system_prompt.trim();
    if !user_prompt.is_empty() {
        openai_messages.push(OpenAiMessage {
            role: "developer".to_string(),
            content: Some(user_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for msg in messages {
        openai_messages.push(OpenAiMessage {
            role: msg.role.clone(),
            content: Some(msg.content.clone()),
            tool_calls: msg
                .tool_calls
                .as_ref()
                .map(|tool_calls| to_openai_tool_calls(tool_calls)),
            tool_call_id: msg.tool_call_id.clone(),
        });
    }

    let request = OpenAiChatRequest {
        model: config.llm_model.clone(),
        messages: openai_messages,
        stream: true,
        max_tokens: 512,
        temperature: 0.6,
        tools: if tools.is_empty() {
            None
        } else {
            Some(tools.to_vec())
        },
        tool_choice: forced_tool.map(|name| {
            serde_json::json!({
                "type": "function",
                "function": { "name": name }
            })
        }),
        chat_template_kwargs: Some(serde_json::json!({ "enable_thinking": false })),
    };

    let resp = client
        .post(openai_chat_completions_url(&config.llm_base_url))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("LLM API error {}: {}", status, body).into());
    }

    use tokio_stream::StreamExt;

    let mut full_response = String::new();
    let mut sentence_buffer = String::new();
    let mut spoken_text = String::new();
    let mut byte_stream = resp.bytes_stream();
    let mut line_buffer = Vec::new();
    let mut tool_call_accumulators: Vec<OpenAiToolCallAccumulator> = Vec::new();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk?;
        line_buffer.extend_from_slice(&chunk);

        while let Some(newline_pos) = line_buffer.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = line_buffer.drain(..=newline_pos).collect();
            let line_str = String::from_utf8_lossy(&line);
            let line_str = line_str.trim();

            if line_str.is_empty() || !line_str.starts_with("data:") {
                continue;
            }

            let data = line_str.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                break;
            }

            let Ok(stream_chunk) = serde_json::from_str::<OpenAiStreamChunk>(data) else {
                continue;
            };

            for choice in stream_chunk.choices {
                let OpenAiDelta {
                    content,
                    tool_calls,
                } = choice.delta;
                collect_openai_tool_call_deltas(&mut tool_call_accumulators, tool_calls);

                if let Some(content) = content {
                    full_response.push_str(&content);
                    sentence_buffer.push_str(&content);

                    while let Some(split_pos) = find_sentence_end(&sentence_buffer) {
                        let sentence: String = sentence_buffer.drain(..=split_pos).collect();
                        let sentence = sentence.trim().to_string();
                        if !sentence.is_empty() {
                            spoken_text.push_str(&sentence);
                            spoken_text.push(' ');
                            let _ = sentence_tx.send(sentence).await;
                        }
                    }
                }
            }
        }
    }

    if !line_buffer.is_empty() {
        let line_str = String::from_utf8_lossy(&line_buffer);
        for raw_line in line_str.lines() {
            let line = raw_line.trim();
            if line.is_empty() || !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                continue;
            }
            if let Ok(stream_chunk) = serde_json::from_str::<OpenAiStreamChunk>(data) {
                for choice in stream_chunk.choices {
                    let OpenAiDelta {
                        content,
                        tool_calls,
                    } = choice.delta;
                    collect_openai_tool_call_deltas(&mut tool_call_accumulators, tool_calls);

                    if let Some(content) = content {
                        full_response.push_str(&content);
                        sentence_buffer.push_str(&content);
                    }
                }
            }
        }
    }

    let tool_calls = openai_tool_call_accumulators_to_ollama(tool_call_accumulators);
    if !tool_calls.is_empty() {
        return Ok(StreamResult::ToolCalls(
            tool_calls,
            spoken_text.trim().to_string(),
            false,
        ));
    }

    let remaining = sentence_buffer.trim().to_string();
    if !remaining.is_empty() {
        let _ = sentence_tx.send(remaining).await;
    }

    Ok(StreamResult::Content(full_response.trim().to_string()))
}

fn to_openai_tool_calls(tool_calls: &[OllamaToolCallOut]) -> Vec<OpenAiToolCallOut> {
    tool_calls
        .iter()
        .enumerate()
        .map(|(index, tool_call)| OpenAiToolCallOut {
            id: tool_call
                .id
                .clone()
                .unwrap_or_else(|| format!("call_{}", index)),
            kind: "function".to_string(),
            function: OpenAiToolFunctionOut {
                name: tool_call.function.name.clone(),
                arguments: serde_json::to_string(&tool_call.function.arguments)
                    .unwrap_or_else(|_| "{}".to_string()),
            },
        })
        .collect()
}

fn collect_openai_tool_call_deltas(
    accumulators: &mut Vec<OpenAiToolCallAccumulator>,
    deltas: Option<Vec<OpenAiToolCallDelta>>,
) {
    let Some(deltas) = deltas else {
        return;
    };

    for (fallback_index, delta) in deltas.into_iter().enumerate() {
        let index = delta.index.unwrap_or(fallback_index);
        while accumulators.len() <= index {
            accumulators.push(OpenAiToolCallAccumulator::default());
        }

        let acc = &mut accumulators[index];
        if let Some(id) = delta.id {
            acc.id = Some(id);
        }
        if let Some(kind) = delta.kind {
            acc.kind = Some(kind);
        }

        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                acc.name.push_str(&name);
            }
            if let Some(arguments) = function.arguments {
                acc.arguments.push_str(&arguments);
            }
        }

        if let Some(name) = delta.name {
            acc.name.push_str(&name);
        }
        if let Some(arguments) = delta.arguments {
            acc.arguments.push_str(&arguments);
        }
    }
}

fn openai_tool_call_accumulators_to_ollama(
    accumulators: Vec<OpenAiToolCallAccumulator>,
) -> Vec<OllamaToolCall> {
    accumulators
        .into_iter()
        .enumerate()
        .filter_map(|(index, acc)| {
            let name = acc.name.trim().to_string();
            if name.is_empty() {
                return None;
            }

            Some(OllamaToolCall {
                id: Some(acc.id.unwrap_or_else(|| format!("call_{}", index))),
                function: OllamaToolFunction {
                    name,
                    arguments: parse_openai_tool_arguments(&acc.arguments),
                },
            })
        })
        .collect()
}

fn parse_openai_tool_arguments(arguments: &str) -> HashMap<String, serde_json::Value> {
    let arguments = arguments.trim();
    if arguments.is_empty() {
        return HashMap::new();
    }

    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(serde_json::Value::Object(map)) => map.into_iter().collect(),
        Ok(value) => {
            let mut map = HashMap::new();
            map.insert("value".to_string(), value);
            map
        }
        Err(_) => {
            let mut map = HashMap::new();
            map.insert(
                "raw".to_string(),
                serde_json::Value::String(arguments.to_string()),
            );
            map
        }
    }
}

fn trim_base_url(url: &str) -> &str {
    url.trim_end_matches('/')
}

fn openai_chat_completions_url(base_url: &str) -> String {
    let base = trim_base_url(base_url);
    if base.ends_with("/v1") {
        format!("{}/chat/completions", base)
    } else if base.ends_with("/v1/chat/completions") {
        base.to_string()
    } else {
        format!("{}/v1/chat/completions", base)
    }
}

/// Parse XML-style tool calls that some models emit as text.
fn parse_xml_tool_calls(content: &str) -> Option<Vec<OllamaToolCall>> {
    let re_block = regex::Regex::new(r"(?s)<(?:\w+:)?tool_call>(.*?)</(?:\w+:)?tool_call>").ok()?;
    let re_invoke = regex::Regex::new(r#"(?s)<invoke\s+name="([^"]+)">(.*?)</invoke>"#).ok()?;
    let re_param =
        regex::Regex::new(r#"(?s)<parameter\s+name="([^"]+)">(.*?)</parameter>"#).ok()?;

    let mut calls = Vec::new();

    for block in re_block.captures_iter(content) {
        let inner = &block[1];
        for invoke_match in re_invoke.captures_iter(inner) {
            let func_name = invoke_match[1].to_string();
            let params_str = &invoke_match[2];

            let mut arguments = HashMap::new();
            for param in re_param.captures_iter(params_str) {
                let key = param[1].trim().to_string();
                let value = param[2].trim().to_string();
                let json_val = serde_json::from_str::<serde_json::Value>(&value)
                    .unwrap_or(serde_json::Value::String(value));
                arguments.insert(key, json_val);
            }

            calls.push(OllamaToolCall {
                id: None,
                function: OllamaToolFunction {
                    name: func_name,
                    arguments,
                },
            });
        }
    }

    if calls.is_empty() {
        None
    } else {
        Some(calls)
    }
}

/// Find the end of a sentence in the buffer.
/// Returns the byte index of the last char of the sentence (inclusive).
fn find_sentence_end(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();

    for i in 0..chars.len() {
        let (_byte_idx, ch) = chars[i];
        if ch == '.' || ch == '!' || ch == '?' {
            // Check it's not an abbreviation (e.g. "Dr." "Mr." "e.g.")
            // Simple heuristic: must be followed by whitespace or end of text
            let next_idx = i + 1;
            if next_idx < chars.len() {
                let (_, next_ch) = chars[next_idx];
                if next_ch.is_whitespace() {
                    // German ordinal numbers ("der 17. Mai", "3. Kapitel") and
                    // version/section numbers shouldn't be treated as sentence
                    // ends. If a '.' is directly preceded by a digit, skip it.
                    if ch == '.' && i > 0 && chars[i - 1].1.is_ascii_digit() {
                        continue;
                    }
                    // Return the position of the whitespace so we consume it
                    return Some(chars[next_idx].0);
                }
            }
            // If at end of buffer, don't split yet — wait for more tokens
        }
    }
    None
}

// ── TTS (OpenAI-compatible /v1/audio/speech) ──

#[derive(Serialize)]
struct TtsRequest {
    input: String,
    voice: String,
}

pub async fn synthesize(
    config: &VoiceConfig,
    text: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let request = TtsRequest {
        input: text.to_string(),
        voice: config.tts_voice.clone(),
    };

    let resp = client
        .post(format!(
            "{}/v1/audio/speech",
            trim_base_url(&config.tts_url)
        ))
        .json(&request)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("TTS API error {}: {}", status, body).into());
    }

    let audio_bytes = resp.bytes().await?;
    Ok(STANDARD.encode(&audio_bytes))
}
