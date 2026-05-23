//! Verwaltung von CLI-Agent-Sessions über tmux.
//!
//! Jede Agent-Session ist eine benannte tmux-Session. Dexter startet den Agent
//! im interaktiven Modus in einer tmux-Session, öffnet ein gnome-terminal dazu,
//! und schickt Spracheingaben per `tmux send-keys` rein.

use crate::state::AppMode;
use std::path::PathBuf;
use tokio::process::Command;

const SESSION_PREFIX: &str = "dexter-";

fn session_name(mode: &AppMode) -> String {
    format!("{}{}", SESSION_PREFIX, mode)
}

fn agent_command(mode: &AppMode) -> Option<&'static str> {
    match mode {
        AppMode::ClaudeSession => Some("claude"),
        AppMode::CodexSession => Some("codex"),
        AppMode::AgySession => Some("agy"),
        AppMode::OpencodeSession => Some("opencode"),
        AppMode::Chat => None,
    }
}

async fn tmux_session_exists(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

pub async fn ensure_session(mode: &AppMode, working_dir: &PathBuf) -> Result<String, String> {
    let name = session_name(mode);
    let agent = agent_command(mode).ok_or_else(|| format!("Kein Agent für {}", mode))?;

    if tmux_session_exists(&name).await {
        return Ok(name);
    }

    let status = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &name,
            "-x",
            "200",
            "-y",
            "50",
            agent,
        ])
        .current_dir(working_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("tmux konnte nicht gestartet werden: {}", e))?;

    if !status.success() {
        return Err(format!("tmux new-session für {} fehlgeschlagen", agent));
    }

    Ok(name)
}

pub async fn open_terminal(session_name: &str) -> Result<(), String> {
    Command::new("gnome-terminal")
        .args(["--", "tmux", "attach-session", "-t", session_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("gnome-terminal konnte nicht gestartet werden: {}", e))?;
    Ok(())
}

pub async fn send_keys(session_name: &str, text: &str) -> Result<(), String> {
    let status = Command::new("tmux")
        .args(["send-keys", "-t", session_name, text, "Enter"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("tmux send-keys fehlgeschlagen: {}", e))?;

    if !status.success() {
        return Err(format!(
            "tmux send-keys an Session '{}' fehlgeschlagen — läuft die Session noch?",
            session_name
        ));
    }
    Ok(())
}

pub async fn kill_session(mode: &AppMode) -> Result<(), String> {
    let name = session_name(mode);
    if !tmux_session_exists(&name).await {
        return Ok(());
    }
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", &name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_format() {
        assert_eq!(session_name(&AppMode::ClaudeSession), "dexter-claude_session");
        assert_eq!(session_name(&AppMode::AgySession), "dexter-agy_session");
    }

    #[test]
    fn agent_command_chat_is_none() {
        assert!(agent_command(&AppMode::Chat).is_none());
    }

    #[test]
    fn agent_command_exists_for_all_sessions() {
        assert_eq!(agent_command(&AppMode::ClaudeSession), Some("claude"));
        assert_eq!(agent_command(&AppMode::CodexSession), Some("codex"));
        assert_eq!(agent_command(&AppMode::AgySession), Some("agy"));
        assert_eq!(agent_command(&AppMode::OpencodeSession), Some("opencode"));
    }
}
