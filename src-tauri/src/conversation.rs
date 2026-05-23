//! Chat-Historie: Redaktion veralteter Tool-Ergebnisse.

use crate::ChatMessage;

const VOLATILE_TOOLS: &[&str] = &["get_current_time", "read_clipboard"];

pub fn redact_stale_tool_results(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let last_user_idx = messages.iter().rposition(|m| m.role == "user").unwrap_or(0);

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
                if msg.content.contains("[Tool result for get_current_time]")
                    || msg.content.contains("[Tool result for read_clipboard]")
                {
                    return ChatMessage {
                        content: "(outdated)".to_string(),
                        ..msg.clone()
                    };
                }
            }
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
