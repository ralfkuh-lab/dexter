//! Deterministischer Parser für KOMMANDO-Präfix in STT-Transkripten.

use crate::state::AppMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SetMode(AppMode),
    Status,
    ToggleDictation,
}

const KOMMANDO_PREFIXES: &[&str] = &[
    "kommando",
    "komando",
    "commando",
    "comando",
    "command",
    "dexter kommando",
    "dexter komando",
    "dexter commando",
    "dexter comando",
    "dexter command",
];

pub fn parse(transcript: &str) -> Option<Command> {
    let text = transcript.trim().to_lowercase();

    let rest = KOMMANDO_PREFIXES
        .iter()
        .filter_map(|prefix| text.strip_prefix(prefix))
        .map(|rest| rest.trim())
        .next()?;

    match rest {
        "chat" => Some(Command::SetMode(AppMode::Chat)),
        "status" => Some(Command::Status),
        "diktat" | "diktieren" | "dictation" | "diktier" | "dictat" => {
            Some(Command::ToggleDictation)
        }
        _ if matches_coding_session(rest, "codex") => {
            Some(Command::SetMode(AppMode::CodexSession))
        }
        _ if matches_coding_session(rest, "claude") => {
            Some(Command::SetMode(AppMode::ClaudeSession))
        }
        _ if matches_coding_session(rest, "opencode") || matches_coding_session(rest, "open code") => {
            Some(Command::SetMode(AppMode::OpencodeSession))
        }
        _ if matches_coding_session(rest, "agy")
            || matches_coding_session(rest, "agi")
            || matches_coding_session(rest, "agee")
            || matches_coding_session(rest, "antigravity") =>
        {
            Some(Command::SetMode(AppMode::AgySession))
        }
        _ => None,
    }
}

fn matches_coding_session(rest: &str, agent: &str) -> bool {
    let patterns = [
        format!("coding session {agent}"),
        format!("coding-session {agent}"),
        format!("session {agent}"),
        format!("{agent} session"),
        format!("{agent}"),
    ];
    patterns.iter().any(|p| rest == p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chat() {
        assert_eq!(parse("Kommando Chat"), Some(Command::SetMode(AppMode::Chat)));
    }

    #[test]
    fn parse_status() {
        assert_eq!(parse("KOMMANDO Status"), Some(Command::Status));
    }

    #[test]
    fn parse_coding_session_codex() {
        assert_eq!(
            parse("Kommando Coding Session Codex"),
            Some(Command::SetMode(AppMode::CodexSession))
        );
    }

    #[test]
    fn parse_coding_session_claude() {
        assert_eq!(
            parse("dexter kommando session claude"),
            Some(Command::SetMode(AppMode::ClaudeSession))
        );
    }

    #[test]
    fn parse_coding_session_agy() {
        assert_eq!(
            parse("kommando agy"),
            Some(Command::SetMode(AppMode::AgySession))
        );
        assert_eq!(
            parse("kommando coding session antigravity"),
            Some(Command::SetMode(AppMode::AgySession))
        );
    }

    #[test]
    fn parse_coding_session_opencode() {
        assert_eq!(
            parse("kommando session opencode"),
            Some(Command::SetMode(AppMode::OpencodeSession))
        );
        assert_eq!(
            parse("kommando coding session open code"),
            Some(Command::SetMode(AppMode::OpencodeSession))
        );
    }

    #[test]
    fn parse_stt_typo_komando() {
        assert_eq!(
            parse("Komando Chat"),
            Some(Command::SetMode(AppMode::Chat))
        );
    }

    #[test]
    fn parse_commando_variant() {
        assert_eq!(
            parse("Commando Status"),
            Some(Command::Status)
        );
    }

    #[test]
    fn parse_unknown_command_returns_none() {
        assert_eq!(parse("Kommando fliegendes Einhorn"), None);
    }

    #[test]
    fn parse_no_prefix_returns_none() {
        assert_eq!(parse("Wie wird das Wetter morgen?"), None);
    }

    #[test]
    fn parse_bare_kommando_returns_none() {
        assert_eq!(parse("Kommando"), None);
    }
}
