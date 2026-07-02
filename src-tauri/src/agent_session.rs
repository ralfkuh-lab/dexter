//! Verwaltung von CLI-Agent-Sessions über tmux.
//!
//! Alle Agenten leben als **Panes** in EINER gemeinsamen tmux-Session
//! (`dexter-agents`), gekachelt im `tiled`-Layout. Ein einziges Terminal-Fenster
//! attached an diese Session zeigt damit alle aktiven Agenten nebeneinander.
//! Jeder Pane wird über die Pane-User-Option `@dexter_agent` seinem AppMode
//! zugeordnet, sodass Spracheingaben per `tmux send-keys` an den richtigen Pane
//! gehen und die Zuordnung auch einen Dexter-Neustart übersteht.

use crate::state::{AppMode, AppState};
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};
use tokio::process::Command;

/// Gemeinsame tmux-Session, in der alle Agent-Panes leben.
const AGENTS_SESSION: &str = "dexter-agents";
/// Pane-User-Option, die einen Pane seinem Agent-Modus zuordnet.
const PANE_OPT: &str = "@dexter_agent";

fn agent_command(mode: &AppMode) -> Option<&'static str> {
    match mode {
        AppMode::ClaudeSession => Some("claude"),
        AppMode::CodexSession => Some("codex"),
        AppMode::AgySession => Some("agy"),
        AppMode::OpencodeSession => Some("opencode"),
        AppMode::Chat => None,
    }
}

/// Wert der Pane-Option für einen Modus (z. B. "claude_session").
fn agent_tag(mode: &AppMode) -> String {
    mode.to_string()
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

pub struct SessionInfo {
    /// Name der gemeinsamen Agents-Session (Ziel für `open_terminal`).
    pub name: String,
    /// tmux-Pane-ID des Agenten (Ziel für `send_keys`/`send_enter`).
    pub pane_id: String,
    /// True, wenn der Pane in diesem Aufruf neu erstellt wurde.
    pub created: bool,
}

/// Sucht die Pane-ID eines Agenten in der gemeinsamen Session anhand seiner
/// `@dexter_agent`-Markierung. `None`, wenn kein passender Pane existiert.
async fn find_pane(tag: &str) -> Option<String> {
    let out = Command::new("tmux")
        .args([
            "list-panes",
            "-t",
            AGENTS_SESSION,
            "-F",
            "#{pane_id} #{@dexter_agent}",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.splitn(2, ' ');
        let id = parts.next().unwrap_or("");
        let this_tag = parts.next().unwrap_or("");
        if this_tag == tag && !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

/// Überwacht den Pane des aktiven Agent-Modus und kehrt nach zwei
/// aufeinanderfolgenden fehlgeschlagenen Prüfungen in den Chat-Modus zurück.
pub fn spawn_lifecycle_watcher(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut watched_mode: Option<AppMode> = None;
        let mut missing_ticks = 0_u8;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            let mode = {
                let state = app.state::<AppState>();
                let mode = state.app_mode.lock().unwrap().clone();
                mode
            };

            if mode == AppMode::Chat {
                watched_mode = None;
                missing_ticks = 0;
                continue;
            }

            if watched_mode.as_ref() != Some(&mode) {
                watched_mode = Some(mode.clone());
                missing_ticks = 0;
            }

            if find_pane(&agent_tag(&mode)).await.is_some() {
                missing_ticks = 0;
                continue;
            }

            missing_ticks += 1;
            if missing_ticks < 2 {
                continue;
            }

            let switched = {
                let state = app.state::<AppState>();
                let mut current_mode = state.app_mode.lock().unwrap();
                if *current_mode == mode {
                    *current_mode = AppMode::Chat;
                    true
                } else {
                    false
                }
            };

            if switched {
                let label = AppMode::Chat.to_string();
                let _ = app.emit("app_mode_changed", &label);
                let _ = app.emit(
                    "assistant_text",
                    &format!("Agent-Session {} beendet — zurück im Chat-Modus", mode),
                );
            }

            watched_mode = None;
            missing_ticks = 0;
        }
    });
}

/// Markiert einen Pane mit dem Agent-Tag, damit er später wiederfindbar ist.
async fn set_pane_tag(pane_id: &str, tag: &str) {
    let _ = Command::new("tmux")
        .args(["set-option", "-p", "-t", pane_id, PANE_OPT, tag])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
}

/// Kachelt alle Panes der Session gleichmäßig (tiled).
async fn retile() {
    let _ = Command::new("tmux")
        .args(["select-layout", "-t", AGENTS_SESSION, "tiled"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
}

/// Hebt einen Pane als aktiven hervor (farbiger Rand im Terminal).
async fn focus_pane(pane_id: &str) {
    let _ = Command::new("tmux")
        .args(["select-pane", "-t", pane_id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
}

/// Startet die gemeinsame Session mit dem ersten Agent-Pane und gibt dessen
/// Pane-ID zurück.
async fn new_session_pane(agent: &str, working_dir: &str) -> Result<String, String> {
    let out = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            AGENTS_SESSION,
            "-x",
            "250",
            "-y",
            "50",
            "-c",
            working_dir,
            "-P",
            "-F",
            "#{pane_id}",
            agent,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("tmux new-session fehlgeschlagen: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "tmux new-session für {} fehlgeschlagen: {}",
            agent,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Splittet einen neuen Agent-Pane in die bestehende Session und gibt dessen
/// Pane-ID zurück.
async fn split_pane(agent: &str, working_dir: &str) -> Result<String, String> {
    let out = Command::new("tmux")
        .args([
            "split-window",
            "-t",
            AGENTS_SESSION,
            "-c",
            working_dir,
            "-P",
            "-F",
            "#{pane_id}",
            agent,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("tmux split-window fehlgeschlagen: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "tmux split-window für {} fehlgeschlagen: {}",
            agent,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Stellt sicher, dass für `mode` ein Agent-Pane in der gemeinsamen Session
/// existiert (erstellt ihn bei Bedarf), kachelt neu und fokussiert ihn.
pub async fn ensure_session(mode: &AppMode, working_dir: &PathBuf) -> Result<SessionInfo, String> {
    let agent = agent_command(mode).ok_or_else(|| format!("Kein Agent für {}", mode))?;
    let tag = agent_tag(mode);
    let dir = working_dir.to_string_lossy().to_string();

    let (pane_id, created) = if !tmux_session_exists(AGENTS_SESSION).await {
        let id = new_session_pane(agent, &dir).await?;
        set_pane_tag(&id, &tag).await;
        (id, true)
    } else if let Some(id) = find_pane(&tag).await {
        (id, false)
    } else {
        let id = split_pane(agent, &dir).await?;
        set_pane_tag(&id, &tag).await;
        retile().await;
        (id, true)
    };

    focus_pane(&pane_id).await;

    Ok(SessionInfo {
        name: AGENTS_SESSION.to_string(),
        pane_id,
        created,
    })
}

async fn session_is_attached(name: &str) -> bool {
    Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name} #{session_attached}"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .map(|out| {
            String::from_utf8_lossy(&out.stdout).lines().any(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                parts.len() == 2 && parts[0] == name && parts[1] != "0"
            })
        })
        .unwrap_or(false)
}

fn effective_terminal(terminal: &str) -> &str {
    let terminal = terminal.trim();
    if terminal.is_empty() {
        "gnome-terminal"
    } else {
        terminal
    }
}

fn terminal_args(terminal: &str, session_name: &str) -> Vec<String> {
    let binary = Path::new(effective_terminal(terminal))
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    let args = match binary {
        "gnome-terminal" | "xfce4-terminal" => {
            vec!["--", "tmux", "attach-session", "-t", session_name]
        }
        "wezterm" => vec!["start", "--", "tmux", "attach-session", "-t", session_name],
        "kitty" => vec!["tmux", "attach-session", "-t", session_name],
        _ => vec!["-e", "tmux", "attach-session", "-t", session_name],
    };

    args.into_iter().map(String::from).collect()
}

pub async fn open_terminal(session_name: &str, terminal: &str) -> Result<(), String> {
    if session_is_attached(session_name).await {
        return Ok(());
    }
    let terminal = effective_terminal(terminal);
    Command::new(terminal)
        .args(terminal_args(terminal, session_name))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "Terminal '{}' konnte nicht gestartet werden: {}",
                terminal, e
            )
        })?;
    Ok(())
}

pub async fn send_keys(pane: &str, text: &str) -> Result<(), String> {
    let text_status = Command::new("tmux")
        .args(["send-keys", "-t", pane, "-l", text])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("tmux send-keys fehlgeschlagen: {}", e))?;

    if !text_status.success() {
        return Err(format!(
            "tmux send-keys an Pane '{}' fehlgeschlagen — läuft der Agent noch?",
            pane
        ));
    }

    send_enter(pane).await
}

pub async fn send_enter(pane: &str) -> Result<(), String> {
    let status = Command::new("tmux")
        .args(["send-keys", "-t", pane, "Enter"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()
        .await
        .map_err(|e| format!("tmux send-keys fehlgeschlagen: {}", e))?;

    if !status.success() {
        return Err(format!(
            "tmux send-keys an Pane '{}' fehlgeschlagen — läuft der Agent noch?",
            pane
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_tag_matches_mode_display() {
        assert_eq!(agent_tag(&AppMode::ClaudeSession), "claude_session");
        assert_eq!(agent_tag(&AppMode::AgySession), "agy_session");
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

    #[test]
    fn terminal_args_use_double_dash_for_gnome_and_xfce() {
        let expected = vec!["--", "tmux", "attach-session", "-t", "agents"];
        assert_eq!(terminal_args("gnome-terminal", "agents"), expected);
        assert_eq!(terminal_args("xfce4-terminal", "agents"), expected);
        assert_eq!(terminal_args("", "agents"), expected);
    }

    #[test]
    fn terminal_args_handle_wezterm_and_kitty() {
        assert_eq!(
            terminal_args("wezterm", "agents"),
            vec!["start", "--", "tmux", "attach-session", "-t", "agents"]
        );
        assert_eq!(
            terminal_args("/usr/bin/kitty", "agents"),
            vec!["tmux", "attach-session", "-t", "agents"]
        );
    }

    #[test]
    fn terminal_args_use_exec_flag_for_other_terminals() {
        let expected = vec!["-e", "tmux", "attach-session", "-t", "agents"];
        assert_eq!(terminal_args("alacritty", "agents"), expected);
        assert_eq!(terminal_args("/usr/bin/konsole", "agents"), expected);
        assert_eq!(terminal_args("xterm", "agents"), expected);
    }
}
