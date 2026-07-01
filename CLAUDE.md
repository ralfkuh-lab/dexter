# CLAUDE.md

Repo-spezifische Konventionen für Claude. Wird automatisch in jede Session geladen.

## Was dieses Projekt ist

Dexter — Desktop-Sprachassistent als Tauri-App (Rust + React + TypeScript). Die App
ist ein **Client**: sie nimmt Audio auf, schickt es per HTTP an einen Whisper-STT-
Server, schickt den Text an einen Ollama-/llama.cpp-LLM-Server, streamt die Antwort
satzweise an einen Piper-TTS-Server und spielt die WAV-Chunks ab. Plus RAG (lokales
SQLite) und Tools (Screenshot, Clipboard, Shell-Sandbox, Web-Fetch, …).

**Wichtig:** Dexter soll beim Coding **nicht** selbst programmieren und nicht das
lokale Gemma-Modell als Coding-Agent verwenden. Dexter ist Voice-Orchestrator und
Bedienoberfläche für externe CLI-Agenten wie Codex CLI, Claude Code CLI, opencode,
agy. Fachliche Coding-Arbeit, Reviews, Refactorings und Commits sollen bei diesen
CLI-Agenten bleiben. Dexter routet Sprache, zeigt Output, reicht Rückfragen durch
und kann einfache manuelle Hilfsaktionen wie Build/Test/App-Start anstoßen.

## Projektziel / Vision

**Endziel:** Voice-First Desktop Control — der Nutzer kann komplett per Sprache
seinen Rechner bedienen, inklusive Coding-Sessions, ohne Tastatur oder Maus.
Ausführliche Vision in **`docs/VISION.md`**. Jede Session sollte das Endziel im
Blick behalten und Features darauf ausrichten.

**Wichtig:** Die Server-Stack-Infrastruktur (Whisper, llama.cpp, Piper) liegt in
einem **separaten Repo**: `/home/ralf/dev/local-ai/voice-assistant-stack/`. Dort
sind `start-stack.sh`, `stop-stack.sh`, `status.sh` und Docs zu Modellwahl,
VRAM-Budget, Endpunkten. Nichts davon hier ins Repo ziehen.

## Architektur

### Application Modes

Dexter hat mehrere Modi (`AppMode` in `state.rs`):
- **Chat** (Default): Spracheingabe geht ans lokale LLM (Gemma).
- **CodexSession / ClaudeSession / OpencodeSession / AgySession**: Spracheingabe
  wird per `tmux send-keys` an den jeweiligen CLI-Agenten in einem Terminal
  weitergeleitet.

Modus-Wechsel per Sprache: `"Kommando Session Claude"`, `"Kommando Chat"` etc.
Der KOMMANDO-Präfix wird deterministisch geparst (`command_parser.rs`), nie ans LLM
geschickt.

### Agent-Sessions (tmux)

CLI-Agenten laufen in benannten tmux-Sessions (`dexter-<mode>`). Bei Modus-Wechsel
startet Dexter die tmux-Session + ein gnome-terminal dazu. Spracheingaben gehen per
`tmux send-keys` rein. Sessions bleiben beim Zurückwechseln zu Chat im Hintergrund.

### Backend-Module

```
src-tauri/src/
  lib.rs              Tauri-Setup, Tray, Hotkeys, AppState-Init, Command-Registry
  pipeline.rs         PTT-Handler, STT→LLM→TTS Orchestrierung (~580 Zeilen)
  dictation.rs        Diktier-Modus: Multi-Segment-Spracheingabe mit Buffer
  agent_session.rs    tmux-basierte CLI-Agent-Sessions
  command_parser.rs   Deterministischer KOMMANDO-Parser (STT-tolerant)
  dialog_manager.rs   ask_user-Dialoge: Sprach-/Klick-Auflösung, Timeout
  panel_manager.rs    Panel-Fenster, UI-Sprachbefehle, UI-Kontext
  tool_executor.rs    Tool-Dispatch und Ausführung aller LLM-Tools
  conversation.rs     Chat-Historie, Redaktion veralteter Tool-Ergebnisse
  automation.rs       Lokale Test-/Steuerungs-API (127.0.0.1:9877)
  state.rs            AppState, AppMode, Dialog/Panel/Processing State
  config.rs           ToolsConfig, VoiceConfig (System-Prompt wird aus system-prompt.md geladen)
  voice.rs            HTTP-STT, LLM-Streaming (Ollama+OpenAI), TTS, Tool-Defs
  tools.rs            Tool-Implementierungen (cfg-getrennt pro OS)
  sandbox.rs          Shell-Sandbox für run_command
  rag.rs              Lokale RAG (SQLite + Ollama-Embeddings)
  backend.rs          LLM-Warmup, PTT-Shortcut, ctx_max Discovery
  window.rs           Fenster-Utilities
```

### Frontend-Module

```
src/
  App.tsx             URL-Routing (main/settings/panel), Automation Console
  orb/
    Orb.tsx           Haupt-UI: Chat-Bubbles, PTT, Dialog, Toolbar, Orb
    ModeBar.tsx       Farbkodierter AppMode-Indikator (nur in Session-Modi)
    Bubble.tsx        Chat-Bubble-Renderer (user/assistant/tool/debug/status)
    StatsBar.tsx      Model-Info + Performance-Metriken
  settings/
    Settings.tsx      Settings-Shell mit Tab-Navigation
    ConfigTab.tsx     Server/Modell/TTS/Hotkey-Konfiguration
    PromptTab.tsx     System-Prompt-Editor
    ToolsTab.tsx      Tool-Toggles + Sandbox-Modus
    KnowledgeTab.tsx  Knowledge-Base-Verwaltung
  panel/
    Panel.tsx         Detail-Panel für Markdown-Output
  components/
    ui.tsx            Wiederverwendbare Form-Komponenten
    ModelSelect.tsx   Modell-Dropdown + Autocomplete
  automation/
    console.ts        Frontend-Fehler ans Backend melden
```

### Automation API (127.0.0.1:9877)

Lokale HTTP-API für E2E-Tests und Scripting. Endpunkte:
- `GET /state` — Kompakter App-Zustand inkl. `app_mode`
- `GET /events` — Automation-Event-Log
- `POST /text` — Text-Eingabe (wie Tastatur)
- `POST /ptt/press|release|cancel` — PTT-Steuerung
- `POST /dialog/answer` — Dialog beantworten
- `POST /panel/close` — Panel schließen
- `POST /wait` — Auf Bedingung warten (idle, recording, dialog.shown etc.)
- `POST /quit` — App beenden

E2E-Smoke: `tests/e2e/smoke_automation.py`

### System-Prompt

Der System-Prompt liegt als **`system-prompt.md`** im Projekt-Root (nicht mehr
hartcodiert in config.rs). Änderungen werden beim App-Neustart geladen.

### Tool-Calling-Optimierung

Kleine LLMs (4-8B) rufen Tools oft nicht zuverlässig auf. Strategie-Dokument
in `docs/TOOL-CALLING-STRATEGY.md`. Test-Harness: `tests/tool_calling/eval.py`
mit Szenarien in `tests/tool_calling/scenarios.json`. Referenz-Implementierung
für Rescue Parsing und Retry Nudging: `/home/ralf/dev/forge`.

## Multi-Platform-Ambition, Linux-Realität

Ziel ist macOS/Linux/Windows. Aktive Entwicklung läuft auf **Linux Mint** (Lab-
Hardware: RTX 3060 Laptop, 6 GB VRAM). macOS-Code in `tools.rs`/`sandbox.rs`/
`lib.rs` ist hinter `#[cfg(target_os = "macos")]` gegated, bleibt drin, aber
ungetestet. Windows-Stubs existieren teils. **Nie** macOS-Pfade rausreißen ohne
explizite Ansage.

## Entwickeln & Starten

```bash
./scripts/dev-linux.sh    # vite + cargo tauri dev (hot-reload für UI)
./scripts/run-linux.sh    # nur das gebaute Debug-Binary
```

`dev-linux.sh` setzt `PKG_CONFIG_PATH`/`CFLAGS`/`LD_LIBRARY_PATH` auf ALSA aus
`~/.cache/dexter-deps/alsa/root/` — wird von `cpal` (Mikrofon) gebraucht.

Häufige Stolperfalle: alter Vite-Dev-Server-Zombie auf Port 1420. Wenn `dev-linux.sh`
mit „Port 1420 already in use" stirbt: `pgrep -f vite` + kill.

Cargo-Check/Test mit ALSA-Env:
```bash
cd src-tauri
export DEXTER_ALSA="${DEXTER_ALSA:-$HOME/.cache/dexter-deps/alsa/root}"
export PKG_CONFIG_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export CFLAGS="-I$DEXTER_ALSA/usr/include ${CFLAGS:-}"
export LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu${LIBRARY_PATH:+:$LIBRARY_PATH}"
export LD_LIBRARY_PATH="$DEXTER_ALSA/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
cargo check
cargo test
```

## Stack starten (bevor Dexter sinnvoll testbar ist)

```bash
cd /home/ralf/dev/local-ai/voice-assistant-stack
./start-stack.sh        # llama + piper. Whisper läuft separat als systemd-Dienst.
./status.sh             # Health-Check aller drei
```

Dexter erwartet die drei Endpunkte unter:
- STT: `http://127.0.0.1:8350` (faster-whisper, separates Repo: `~/dev/Whisper-dictate`)
- LLM: `http://127.0.0.1:8081` (llama.cpp Docker)
- TTS: `http://127.0.0.1:8005` (Piper, Script + venv leben hier in `scripts/`)

## Konventionen

- **Alles per Settings-UI konfigurierbar:** Jede Einstellung, die das Verhalten
  steuert (Server-URLs, Modelle, TTS, Hotkeys, Sandbox inkl. `readable_paths`,
  Tool-Toggles, …), muss über die Settings-Tabs bedienbar sein. Der User will
  **keine** Config-Dateien von Hand pflegen. Wer ein neues Config-Feld einführt
  oder ein bestehendes sicherheitsrelevant macht, muss es auch in der UI
  (`src/settings/`) exponieren.
- **User-Config:** liegt in `~/.config/voice-assistant/config.json`. Existiert
  per Default nicht, dann greifen die Defaults aus `lib.rs::VoiceConfig::default()`.
  **Wichtig:** Wenn der User die Settings einmal speichert, sind die Werte
  eingefroren — Default-Updates greifen für ihn nicht mehr.
- **`serde(alias = "...")`** verwenden bei Config-Feld-Umbenennungen, damit
  alte gespeicherte Configs weiter geladen werden.
- **Voices/venvs nie committen** — sind in `.gitignore`, ~60 MB binary.
- **System-Prompt ist Deutsch** und enthält eine explizite Anweisung „antworte
  immer auf Deutsch". Bei Änderung nicht versehentlich wieder Englisch machen.
- **Keine paralinguistic Tags** (`[laugh]`, `[sigh]` etc.) im System-Prompt —
  Piper liest die vor.

## Git / Commit

- Commit-Messages: Imperativ, ein klarer Titel, Body erklärt **warum**.
  Keinen Co-Authored-By-Trailer anhängen.
- Branch: alles auf `main`, kein PR-Workflow. Direkt commit + push.

## TODO

`TODO.md` ist das aktive Backlog. Erledigte Items raus (nicht abhaken-und-
stehenlassen). Wenn ich was offen lasse, gehört ein Eintrag dort rein.
