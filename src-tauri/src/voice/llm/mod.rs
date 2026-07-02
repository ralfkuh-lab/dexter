//! OpenAI-kompatibler Streaming-Chat-Client plus geteilte
//! Hilfen für Satz-Splitting und XML-Tool-Call-Parsing.

use crate::{ChatMessage, VoiceConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;

mod openai;

/// Serializable tool call (for sending back in assistant messages).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolCallOut {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    pub function: ToolFunctionOut,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolFunctionOut {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ToolCall {
    #[serde(default)]
    pub id: Option<String>,
    pub function: ToolFunction,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

impl ToolCall {
    /// Convert deserialized tool call to serializable form for echoing back.
    pub fn to_out(&self) -> ToolCallOut {
        ToolCallOut {
            id: self.id.clone(),
            function: ToolFunctionOut {
                name: self.function.name.clone(),
                arguments: self.function.arguments.clone(),
            },
        }
    }
}

/// Origin of the tool calls in `StreamResult::ToolCalls`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCallSource {
    /// Provider's native structured tool-call format. Echoed back to the LLM as-is.
    Native,
    /// Model emitted text-mode XML tool calls (`<tool_call>…</tool_call>`)
    /// that we parsed out and rewrote as native form.
    Xml,
}

/// Result of a streaming chat — either the model streamed content (sentences sent
/// via channel) or it requested tool calls.
pub enum StreamResult {
    /// Model streamed a text response. Full text returned here.
    Content(String),
    /// Model requested tool calls. `spoken_preamble` is any pre-tool-call
    /// narration text (e.g. "Let me search for that") that was already sent
    /// to TTS before the tool call started.
    ToolCalls {
        calls: Vec<ToolCall>,
        spoken_preamble: String,
        source: ToolCallSource,
    },
}

/// Unified streaming chat. Streams with tools enabled.
/// - If the model produces content tokens → sentences are sent via `sentence_tx`, returns `StreamResult::Content`.
/// - If the model returns tool_calls → returns `StreamResult::ToolCalls` (nothing sent via channel).
/// - Also handles XML-style tool calls from models that don't use native format.
pub async fn chat_streaming(
    app: &tauri::AppHandle,
    config: &VoiceConfig,
    messages: &[ChatMessage],
    tools: &[serde_json::Value],
    forced_tool: Option<&str>,
    sentence_tx: &mpsc::Sender<String>,
) -> Result<StreamResult, Box<dyn std::error::Error + Send + Sync>> {
    openai::chat_streaming(app, config, messages, tools, forced_tool, sentence_tx).await
}

/// Fire-and-forget warmup: send the static prompt prefix (system + developer)
/// plus a tiny user turn with max_tokens=1 to prime the backend's prompt cache.
/// Subsequent real requests with the same prefix skip prompt-eval entirely,
/// which on slow/large models is the dominant TTFT cost. Also parses
/// `prompt_tokens` from the response and emits an llm_stats event so the
/// stats bar shows the static prefix size before the first real request.
pub async fn warmup_llm(app: &tauri::AppHandle, config: &VoiceConfig) {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let result = openai::warmup(&client, config).await;

    let resp = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("LLM warmup failed: {}", e);
            return;
        }
    };

    let status = resp.status();
    if !status.is_success() {
        eprintln!(
            "LLM warmup non-OK: {} ({})",
            status,
            resp.text().await.unwrap_or_default()
        );
        return;
    }

    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return;
    };

    // Find prompt-token count under either OpenAI (usage.prompt_tokens) or
    // Ollama (prompt_eval_count) naming.
    let prompt_tokens = body
        .pointer("/usage/prompt_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| body.get("prompt_eval_count").and_then(|v| v.as_u64()))
        .map(|n| n as u32);

    let model = body.get("model").and_then(|v| v.as_str()).map(String::from);

    let stats = super::LlmStats {
        prompt_tokens,
        model,
        ..Default::default()
    };
    super::emit_llm_stats(app, stats);
}

/// Find the end of a sentence in the buffer.
/// Returns the byte index of the last char of the sentence (inclusive).
pub(super) fn find_sentence_end(text: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();

    for i in 0..chars.len() {
        let (_byte_idx, ch) = chars[i];
        if ch == '.' || ch == '!' || ch == '?' {
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
                    return Some(chars[next_idx].0);
                }
            }
        }
    }
    None
}

/// Parse XML-style tool calls that some models emit as text.
pub(super) fn parse_xml_tool_calls(content: &str) -> Option<Vec<ToolCall>> {
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

            calls.push(ToolCall {
                id: None,
                function: ToolFunction {
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
