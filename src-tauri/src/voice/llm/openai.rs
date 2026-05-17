//! OpenAI-kompatibler `/v1/chat/completions` Streaming-Client (llama.cpp etc.).

use super::{find_sentence_end, OllamaToolCall, OllamaToolCallOut, OllamaToolFunction, StreamResult, ToolCallSource};
use crate::voice::{emit_llm_stats, trim_base_url, LlmStats};
use crate::{core_system_prompt, ChatMessage, VoiceConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;

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
    /// Ask the server to send a final usage chunk in the stream.
    stream_options: OpenAiStreamOptions,
}

#[derive(Serialize)]
struct OpenAiStreamOptions {
    include_usage: bool,
}

#[derive(Deserialize, Default)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
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
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<OpenAiStreamChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
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

pub(super) async fn chat_streaming(
    app: &tauri::AppHandle,
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
        content: Some(core_system_prompt().to_string()),
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
        stream_options: OpenAiStreamOptions { include_usage: true },
    };

    let resp = client
        .post(chat_completions_url(&config.llm_base_url))
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

    let req_start = Instant::now();
    let mut first_token_at: Option<Instant> = None;
    let mut stats = LlmStats::default();

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

            if stats.model.is_none() {
                if let Some(m) = &stream_chunk.model {
                    stats.model = Some(m.clone());
                }
            }

            if let Some(usage) = stream_chunk.usage {
                if usage.prompt_tokens.is_some() {
                    stats.prompt_tokens = usage.prompt_tokens;
                }
                if usage.completion_tokens.is_some() {
                    stats.completion_tokens = usage.completion_tokens;
                }
            }

            for choice in stream_chunk.choices {
                let OpenAiDelta {
                    content,
                    tool_calls,
                } = choice.delta;
                collect_tool_call_deltas(&mut tool_call_accumulators, tool_calls);

                if let Some(content) = content {
                    if !content.is_empty() && first_token_at.is_none() {
                        first_token_at = Some(Instant::now());
                        stats.ttft_ms = Some(req_start.elapsed().as_millis() as u64);
                    }
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

    if let (Some(n), Some(first)) = (stats.completion_tokens, first_token_at) {
        let secs = first.elapsed().as_secs_f64();
        if secs > 0.0 {
            stats.tokens_per_sec = Some(n as f64 / secs);
        }
    }
    emit_llm_stats(app, stats);

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
                    collect_tool_call_deltas(&mut tool_call_accumulators, tool_calls);

                    if let Some(content) = content {
                        full_response.push_str(&content);
                        sentence_buffer.push_str(&content);
                    }
                }
            }
        }
    }

    let tool_calls = accumulators_to_ollama(tool_call_accumulators);
    if !tool_calls.is_empty() {
        return Ok(StreamResult::ToolCalls {
            calls: tool_calls,
            spoken_preamble: spoken_text.trim().to_string(),
            source: ToolCallSource::Native,
        });
    }

    let remaining = sentence_buffer.trim().to_string();
    if !remaining.is_empty() {
        let _ = sentence_tx.send(remaining).await;
    }

    Ok(StreamResult::Content(full_response.trim().to_string()))
}

pub(super) async fn warmup(
    client: &reqwest::Client,
    config: &VoiceConfig,
) -> reqwest::Result<reqwest::Response> {
    let user_prompt = config.system_prompt.trim();
    let mut messages = vec![OpenAiMessage {
        role: "system".to_string(),
        content: Some(core_system_prompt().to_string()),
        tool_calls: None,
        tool_call_id: None,
    }];
    if !user_prompt.is_empty() {
        messages.push(OpenAiMessage {
            role: "developer".to_string(),
            content: Some(user_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    messages.push(OpenAiMessage {
        role: "user".to_string(),
        content: Some(".".to_string()),
        tool_calls: None,
        tool_call_id: None,
    });

    let body = serde_json::json!({
        "model": config.llm_model,
        "messages": messages,
        "stream": false,
        "max_tokens": 1,
        "temperature": 0.0,
    });

    client
        .post(chat_completions_url(&config.llm_base_url))
        .json(&body)
        .send()
        .await
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

fn collect_tool_call_deltas(
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

fn accumulators_to_ollama(accumulators: Vec<OpenAiToolCallAccumulator>) -> Vec<OllamaToolCall> {
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
                    arguments: parse_tool_arguments(&acc.arguments),
                },
            })
        })
        .collect()
}

fn parse_tool_arguments(arguments: &str) -> HashMap<String, serde_json::Value> {
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

fn chat_completions_url(base_url: &str) -> String {
    let base = trim_base_url(base_url);
    if base.ends_with("/v1") {
        format!("{}/chat/completions", base)
    } else if base.ends_with("/v1/chat/completions") {
        base.to_string()
    } else {
        format!("{}/v1/chat/completions", base)
    }
}
