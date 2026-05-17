# CLAUDE.md

Repo-spezifische Konventionen für Claude. Wird automatisch in jede Session geladen.

## Was dieses Projekt ist

Dexter — Desktop-Sprachassistent als Tauri-App (Rust + React + TypeScript). Die App
ist ein **Client**: sie nimmt Audio auf, schickt es per HTTP an einen Whisper-STT-
Server, schickt den Text an einen Ollama-/llama.cpp-LLM-Server, streamt die Antwort
satzweise an einen Piper-TTS-Server und spielt die WAV-Chunks ab. Plus RAG (lokales
SQLite) und Tools (Screenshot, Clipboard, Shell-Sandbox, Web-Fetch, …).

**Wichtig:** Die Server-Stack-Infrastruktur (Whisper, llama.cpp, Piper) liegt in
einem **separaten Repo**: `/home/ralf/dev/local-ai/voice-assistant-stack/`. Dort
sind `start-stack.sh`, `stop-stack.sh`, `status.sh` und Docs zu Modellwahl,
VRAM-Budget, Endpunkten. Nichts davon hier ins Repo ziehen.

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

Cargo-Check mit denselben Env-Vars:
```bash
cd src-tauri && PKG_CONFIG_PATH=... cargo check
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

## Dateistruktur

```
src/
  App.tsx           Orb-UI + Settings-Tabs (Config, Prompt, Tools, Knowledge)
src-tauri/src/
  lib.rs            Tauri-Setup, Tray, Hotkeys, Pipeline-Orchestrierung, Config
  voice.rs          HTTP-STT-Client, LLM-Streaming (Ollama+OpenAI), TTS-Client, Tool-Defs
  tools.rs          Tool-Implementierungen (cfg-getrennt pro OS)
  sandbox.rs        Shell-Sandbox für run_command
  rag.rs            Lokale RAG (SQLite + Ollama-Embeddings)
scripts/
  dev-linux.sh, run-linux.sh    Linux-Build/Run-Wrapper
  piper-openai-server.py        TTS-Server (vom Stack via start-stack.sh gestartet)
piper-voices/       ONNX-Voices (NICHT in git — .gitignore)
.venv-piper/        Python-venv für Piper (NICHT in git)
```

## Konventionen

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
  Co-Authored-By-Trailer für Claude (siehe bisherige Commits als Template).
- Branch: alles auf `main`, kein PR-Workflow. Direkt commit + push.

## TODO

`TODO.md` ist das aktive Backlog. Erledigte Items raus (nicht abhaken-und-
stehenlassen). Wenn ich was offen lasse, gehört ein Eintrag dort rein.
