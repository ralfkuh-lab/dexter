//! Ollama-native `/api/chat` Streaming-Client.

use super::{
    find_sentence_end, parse_xml_tool_calls, OllamaToolCall, OllamaToolCallOut, StreamResult,
    ToolCallSource,
};
use crate::voice::{emit_llm_stats, trim_base_url, LlmStats};
use crate::{ChatMessage, VoiceConfig};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tokio::sync::mpsc;

#[derive(Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCallOut>>,
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
struct OllamaResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    model: Option<String>,
    message: Option<OllamaResponseMessage>,
    done: Option<bool>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
    /// Nanoseconds spent generating completion tokens.
    #[serde(default)]
    eval_duration: Option<u64>,
}

pub(super) async fn chat_streaming(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    sentence_tx: &mpsc::Sender<String>,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    let req_start = Instant::now();
    let mut first_token_at: Option<Instant> = None;
    let mut stats = LlmStats::default();
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
    let has_tools = !tools.is_empty();
    let mut xml_collecting = false;
    let mut xml_buffer = String::new();

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
                if stats.model.is_none() {
                    if let Some(m) = &stream_chunk.model {
                        stats.model = Some(m.clone());
                    }
                }
                if let Some(msg) = &stream_chunk.message {
                    if let Some(tc) = &msg.tool_calls {
                        collected_tool_calls.extend(tc.clone());
                    }

                    if !msg.content.is_empty() {
                        if first_token_at.is_none() {
                            first_token_at = Some(Instant::now());
                            stats.ttft_ms = Some(req_start.elapsed().as_millis() as u64);
                        }
                        full_response.push_str(&msg.content);

                        if xml_collecting {
                            xml_buffer.push_str(&msg.content);

                            if xml_close_re.is_match(&xml_buffer) {
                                let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                    collected_tool_calls.extend(parsed);
                                }
                                xml_buffer.clear();
                                xml_collecting = false;
                            }
                        } else {
                            sentence_buffer.push_str(&msg.content);

                            // Only detect XML tool calls when tools are provided
                            if has_tools && xml_open_re.find(&sentence_buffer).is_some() {
                                let m = xml_open_re.find(&sentence_buffer).unwrap();
                                let before = sentence_buffer[..m.start()].trim().to_string();
                                if !before.is_empty() {
                                    spoken_text.push_str(&before);
                                    spoken_text.push(' ');
                                    let _ = sentence_tx.send(before).await;
                                }
                                let after_tag = &sentence_buffer[m.end()..];
                                xml_buffer = after_tag.to_string();
                                sentence_buffer.clear();
                                xml_collecting = true;

                                if xml_close_re.is_match(&xml_buffer) {
                                    let full_xml = format!("<tool_call>{}</tool_call>", xml_buffer);
                                    if let Some(parsed) = parse_xml_tool_calls(&full_xml) {
                                        collected_tool_calls.extend(parsed);
                                    }
                                    xml_buffer.clear();
                                    xml_collecting = false;
                                }
                            } else {
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
                    stats.prompt_tokens = stream_chunk.prompt_eval_count;
                    stats.completion_tokens = stream_chunk.eval_count;
                    if let (Some(n), Some(ns)) =
                        (stream_chunk.eval_count, stream_chunk.eval_duration)
                    {
                        if ns > 0 {
                            stats.tokens_per_sec = Some(n as f64 * 1_000_000_000.0 / ns as f64);
                        }
                    }
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

    emit_llm_stats(app, stats);

    if !collected_tool_calls.is_empty() {
        return Ok(StreamResult::ToolCalls {
            calls: collected_tool_calls,
            spoken_preamble: spoken_text.trim().to_string(),
            source: ToolCallSource::Native,
        });
    }

    // Last-resort fallback: check full response for XML tool calls we might have missed.
    // Only when tools are provided — otherwise model's XML output is just text.
    if has_tools && !full_response.is_empty() {
        if let Some(parsed) = parse_xml_tool_calls(&full_response) {
            if !parsed.is_empty() {
                return Ok(StreamResult::ToolCalls {
                    calls: parsed,
                    spoken_preamble: spoken_text.trim().to_string(),
                    source: ToolCallSource::Xml,
                });
            }
        }
    }

    Ok(StreamResult::Content(full_response.trim().to_string()))
}

pub(super) async fn warmup(
    client: &reqwest::Client,
    config: &VoiceConfig,
) -> reqwest::Result<reqwest::Response> {
    let messages = vec![
        OllamaMessage {
            role: "system".to_string(),
            content: config.effective_system_prompt(),
            tool_calls: None,
        },
        OllamaMessage {
            role: "user".to_string(),
            content: ".".to_string(),
            tool_calls: None,
        },
    ];

    let body = serde_json::json!({
        "model": config.llm_model,
        "messages": messages,
        "stream": false,
        "options": { "num_predict": 1 },
    });

    client
        .post(format!("{}/api/chat", trim_base_url(&config.llm_base_url)))
        .json(&body)
        .send()
        .await
}
