//! Diktier-Modus: Mehrsegment-Spracheingabe mit Buffer-Aufbau und Sprachkommandos.

use crate::state::{emit_processing, ProcessingState};
use crate::AppState;
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationCommand {
    DeleteWord,
    DeleteSentence,
    DeleteAll,
    NewLine,
    Send,
}

pub fn parse_dictation_command(text: &str) -> Option<DictationCommand> {
    let t = text.trim().to_lowercase();

    if matches!(
        t.as_str(),
        "absenden" | "senden" | "fertig" | "over" | "abschicken" | "send"
    ) {
        return Some(DictationCommand::Send);
    }

    if matches!(
        t.as_str(),
        "neue zeile" | "neuer absatz" | "zeilenumbruch" | "new line"
    ) {
        return Some(DictationCommand::NewLine);
    }

    let delete_prefixes = ["lösche", "lösch", "loesche", "loesch", "lösch", "delete"];

    for prefix in &delete_prefixes {
        if let Some(rest) = t.strip_prefix(prefix) {
            let rest = rest.trim();
            if matches!(rest, "alles" | "alle" | "all" | "buffer" | "puffer") {
                return Some(DictationCommand::DeleteAll);
            }
            if matches!(rest, "satz" | "sentence" | "den satz" | "letzten satz") {
                return Some(DictationCommand::DeleteSentence);
            }
            if matches!(
                rest,
                "wort" | "word" | "das wort" | "letztes wort" | "das letzte wort" | "ein wort"
            ) {
                return Some(DictationCommand::DeleteWord);
            }
        }
    }

    None
}

pub fn activate(app: &tauri::AppHandle) {
    {
        let state = app.state::<AppState>();
        *state.is_recording.lock().unwrap() = false;
        *state.dictation_active.lock().unwrap() = true;
        state.dictation_buffer.lock().unwrap().clear();
    }
    let _ = app.emit("dictation_mode_changed", true);
    let _ = app.emit("dictation_buffer_updated", "");
    let _ = emit_processing(
        app,
        ProcessingState {
            stage: "listening".to_string(),
            text: "Höre zu...".to_string(),
        },
    );
    crate::pipeline::start_dictation_loop(app);
}

pub fn deactivate(app: &tauri::AppHandle) {
    crate::pipeline::stop_dictation_loop(app);
    {
        let state = app.state::<AppState>();
        *state.dictation_active.lock().unwrap() = false;
        state.dictation_buffer.lock().unwrap().clear();
    }
    let _ = app.emit("dictation_mode_changed", false);
    let _ = app.emit("dictation_buffer_updated", "");
    let _ = app.emit(
        "dictation_vad",
        serde_json::json!({ "rms": 0.0, "threshold": 0.0, "speech": false }),
    );
    let _ = emit_processing(
        app,
        ProcessingState {
            stage: "idle".to_string(),
            text: String::new(),
        },
    );
}

pub fn is_active(app: &tauri::AppHandle) -> bool {
    *app.state::<AppState>().dictation_active.lock().unwrap()
}

pub fn get_buffer(app: &tauri::AppHandle) -> String {
    app.state::<AppState>()
        .dictation_buffer
        .lock()
        .unwrap()
        .clone()
}

pub fn set_buffer(app: &tauri::AppHandle, text: &str) {
    *app.state::<AppState>().dictation_buffer.lock().unwrap() = text.to_string();
    let _ = app.emit("dictation_buffer_updated", text);
}

/// Verarbeitet ein STT-Segment: entweder Kommando ausführen oder an Buffer anhängen.
/// Gibt `true` zurück wenn der Buffer gesendet werden soll.
pub fn append_segment(app: &tauri::AppHandle, text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    if let Some(cmd) = parse_dictation_command(trimmed) {
        let state = app.state::<AppState>();
        match cmd {
            DictationCommand::Send => return true,
            DictationCommand::DeleteWord => {
                let mut buf = state.dictation_buffer.lock().unwrap();
                delete_last_word(&mut buf);
                let updated = buf.clone();
                drop(buf);
                let _ = app.emit("dictation_buffer_updated", &updated);
            }
            DictationCommand::DeleteSentence => {
                let mut buf = state.dictation_buffer.lock().unwrap();
                delete_last_sentence(&mut buf);
                let updated = buf.clone();
                drop(buf);
                let _ = app.emit("dictation_buffer_updated", &updated);
            }
            DictationCommand::DeleteAll => {
                state.dictation_buffer.lock().unwrap().clear();
                let _ = app.emit("dictation_buffer_updated", "");
            }
            DictationCommand::NewLine => {
                let mut buf = state.dictation_buffer.lock().unwrap();
                buf.push('\n');
                let updated = buf.clone();
                drop(buf);
                let _ = app.emit("dictation_buffer_updated", &updated);
            }
        }
        return false;
    }

    let state = app.state::<AppState>();
    let mut buf = state.dictation_buffer.lock().unwrap();
    if !buf.is_empty() && !buf.ends_with('\n') {
        buf.push(' ');
    }
    buf.push_str(trimmed);
    let updated = buf.clone();
    drop(buf);
    let _ = app.emit("dictation_buffer_updated", &updated);
    false
}

pub async fn send_buffer(app: &tauri::AppHandle) -> Result<(), String> {
    let text = {
        let state = app.state::<AppState>();
        let mut buf = state.dictation_buffer.lock().unwrap();
        let text = buf.trim().to_string();
        buf.clear();
        text
    };
    let _ = app.emit("dictation_buffer_updated", "");

    if text.is_empty() {
        return Ok(());
    }

    let (config, cancel) = {
        let state = app.state::<AppState>();
        let config = state.config.lock().unwrap().clone();
        let cancel = state.pipeline_cancel.lock().unwrap().clone();
        (config, cancel)
    };

    emit_processing(
        app,
        ProcessingState {
            stage: "thinking".to_string(),
            text: "Processing dictation...".to_string(),
        },
    )
    .map_err(|e: tauri::Error| e.to_string())?;

    crate::pipeline::process_text_input(app.clone(), text, config, cancel).await
}

fn delete_last_word(buf: &mut String) {
    let trimmed = buf.trim_end();
    if trimmed.is_empty() {
        buf.clear();
        return;
    }
    if let Some(pos) = trimmed.rfind(|c: char| c.is_whitespace()) {
        buf.truncate(pos);
        // Trailing space beibehalten für natürlichen Textfluss
    } else {
        buf.clear();
    }
}

fn delete_last_sentence(buf: &mut String) {
    let trimmed = buf.trim_end();
    if trimmed.is_empty() {
        buf.clear();
        return;
    }
    let search = if trimmed.ends_with(|c: char| ".!?".contains(c)) {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };
    if let Some(pos) = search.rfind(|c: char| ".!?".contains(c)) {
        buf.truncate(pos + 1);
    } else {
        buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_send_commands() {
        assert_eq!(
            parse_dictation_command("absenden"),
            Some(DictationCommand::Send)
        );
        assert_eq!(
            parse_dictation_command("Over"),
            Some(DictationCommand::Send)
        );
        assert_eq!(
            parse_dictation_command("FERTIG"),
            Some(DictationCommand::Send)
        );
    }

    #[test]
    fn parse_delete_word() {
        assert_eq!(
            parse_dictation_command("lösche Wort"),
            Some(DictationCommand::DeleteWord)
        );
        assert_eq!(
            parse_dictation_command("lösch das Wort"),
            Some(DictationCommand::DeleteWord)
        );
        assert_eq!(
            parse_dictation_command("loesche Wort"),
            Some(DictationCommand::DeleteWord)
        );
    }

    #[test]
    fn parse_delete_sentence() {
        assert_eq!(
            parse_dictation_command("lösche Satz"),
            Some(DictationCommand::DeleteSentence)
        );
        assert_eq!(
            parse_dictation_command("lösch den Satz"),
            Some(DictationCommand::DeleteSentence)
        );
    }

    #[test]
    fn parse_delete_all() {
        assert_eq!(
            parse_dictation_command("lösche alles"),
            Some(DictationCommand::DeleteAll)
        );
        assert_eq!(
            parse_dictation_command("lösch alles"),
            Some(DictationCommand::DeleteAll)
        );
    }

    #[test]
    fn parse_new_line() {
        assert_eq!(
            parse_dictation_command("neue Zeile"),
            Some(DictationCommand::NewLine)
        );
        assert_eq!(
            parse_dictation_command("neuer Absatz"),
            Some(DictationCommand::NewLine)
        );
    }

    #[test]
    fn parse_regular_text_is_none() {
        assert_eq!(parse_dictation_command("Hallo Welt"), None);
        assert_eq!(parse_dictation_command("Was ist 2 plus 2?"), None);
    }

    #[test]
    fn delete_last_word_works() {
        let mut buf = "Hallo Welt das ist ein Test".to_string();
        delete_last_word(&mut buf);
        assert_eq!(buf, "Hallo Welt das ist ein");
        delete_last_word(&mut buf);
        assert_eq!(buf, "Hallo Welt das ist");
    }

    #[test]
    fn delete_last_word_single() {
        let mut buf = "Hallo".to_string();
        delete_last_word(&mut buf);
        assert_eq!(buf, "");
    }

    #[test]
    fn delete_last_sentence_works() {
        let mut buf = "Erster Satz. Zweiter Satz. Dritter".to_string();
        delete_last_sentence(&mut buf);
        assert_eq!(buf, "Erster Satz. Zweiter Satz.");
        delete_last_sentence(&mut buf);
        assert_eq!(buf, "Erster Satz.");
    }

    #[test]
    fn delete_last_sentence_single() {
        let mut buf = "Nur ein Satz".to_string();
        delete_last_sentence(&mut buf);
        assert_eq!(buf, "");
    }
}
