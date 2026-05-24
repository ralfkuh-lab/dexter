//! Dialog-Lifecycle: ask_user-Rückfragen per Sprache oder Klick auflösen.

use crate::state::{emit_processing, DialogOption, ProcessingState};
use crate::AppState;
use tauri::{Emitter, Manager};

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

pub fn handle_pending_dialog_answer(app: &tauri::AppHandle, transcript: &str) -> bool {
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

pub fn clear_pending_dialog(app: &tauri::AppHandle, question: &str) {
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

pub fn match_dialog_selection(transcript: &str, options: &[DialogOption]) -> Option<String> {
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
