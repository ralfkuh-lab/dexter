# Dexter — Desktop Voice Assistant

A local, privacy-first desktop voice assistant. Hold a hotkey, speak, and get a spoken response — all running on your machine. Route voice to CLI coding agents (Claude Code, Codex, agy, opencode) via tmux for hands-free development.

Built with **Tauri 2** (Rust + React). Speaks to local **Whisper** (STT), **Ollama / llama.cpp** (LLM) and **Piper** (TTS) over HTTP. The model servers live in a separate stack project (`voice-assistant-stack`).

## How It Works

```
You speak ──► Whisper transcribes ──► LLM thinks ──► Piper speaks back
               (HTTP STT)              (HTTP LLM)     (HTTP TTS)
```

1. **Hold hotkey** (configurable, default F9) — the orb starts listening
2. **Release** — audio is sent to the Whisper HTTP server for transcription
3. **The LLM generates a response** — streamed sentence-by-sentence for low latency
4. **Each sentence is sent to the TTS server** — audio plays back as chunks arrive

## Application Modes

Dexter has two kinds of modes, switched by voice commands:

| Mode | Voice Command | Behavior |
|------|--------------|----------|
| **Chat** (default) | "Kommando Chat" | Voice goes to local LLM (Gemma) |
| **Claude Session** | "Kommando Session Claude" | Voice goes to Claude Code CLI |
| **Codex Session** | "Kommando Session Codex" | Voice goes to Codex CLI |
| **agy Session** | "Kommando Session agy" | Voice goes to agy CLI |
| **opencode Session** | "Kommando Session opencode" | Voice goes to opencode CLI |

In session modes, Dexter starts the CLI agent in a **tmux session** with a visible terminal window. Voice input is sent via `tmux send-keys` — you see the agent working in real time.

The `KOMMANDO` prefix is parsed deterministically (tolerant of STT typos like "Komando", "Commando", etc.) and never sent to the LLM.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  System Tray                                            │
│  ┌───────────────────────────────────┐                  │
│  │  Tauri (Rust backend)             │                  │
│  │  ├── Pipeline (STT→LLM→TTS)      │                  │
│  │  ├── Command Parser (KOMMANDO)    │                  │
│  │  ├── Agent Sessions (tmux)        │                  │
│  │  ├── Tool Executor (10 tools)     │                  │
│  │  ├── Dialog / Panel Manager       │                  │
│  │  ├── Automation API (:9877)       │                  │
│  │  └── RAG Store (SQLite)           │                  │
│  └───────────────┬───────────────────┘                  │
│                  │ events + invoke                       │
│  ┌───────────────▼───────────────────┐                  │
│  │  React frontend                   │                  │
│  │  ├── Orb UI (chat bubbles)        │                  │
│  │  ├── ModeBar (session indicator)  │                  │
│  │  ├── Settings (tabbed window)     │                  │
│  │  └── Detail Panel (markdown)      │                  │
│  └───────────────────────────────────┘                  │
└─────────────────────────────────────────────────────────┘
```

## Tool Calling

The LLM has access to tools it can invoke mid-conversation (up to 5 rounds):

| Tool | What It Does |
|------|-------------|
| **Screenshot** | Captures screen, sends to vision model for description |
| **Read Clipboard** | Reads clipboard text |
| **Knowledge Search** | Vector similarity search over ingested documents |
| **Open URL** | Opens a URL in the default browser |
| **Current Time** | Returns current date/time |
| **Running Apps** | Lists foreground applications |
| **Web Fetch** | Fetches and extracts text from a URL |
| **Show Panel** | Opens a detail window with markdown content |
| **Ask User** | Shows a multiple-choice dialog, waits for voice/click answer |
| **Run Command** | Executes a shell command in a guarded sandbox |

All tools can be individually enabled/disabled from Settings > Tools.

## The Orb

The UI is a frameless transparent window with a glowing orb. Chat bubbles stack upward. A color-coded mode bar appears when in a CLI agent session.

| Color | State |
|-------|-------|
| Blue (breathing) | Idle — ready |
| Red (pulsing) | Listening — recording |
| Amber (spinning) | Processing — transcribing |
| Purple (spinning) | Thinking — waiting for LLM |
| Cyan (pulsing) | Speaking — playing TTS |
| Dim red | Error |

## Automation API

Local HTTP API on `127.0.0.1:9877` for E2E testing and scripting:

```bash
curl http://127.0.0.1:9877/state                                          # App state
curl -X POST http://127.0.0.1:9877/text -H 'content-type: application/json' -d '{"text":"Hello"}'
curl -X POST http://127.0.0.1:9877/quit -H 'content-type: application/json' -d '{}'
```

## Prerequisites

- **Whisper STT server** on port 8350
- **LLM server** (Ollama or llama.cpp) on port 8081
- **TTS server** (Piper, OpenAI-compatible) on port 8005
- **tmux** for CLI agent sessions

All three model servers are typically managed by the separate `voice-assistant-stack` project.

## Build & Run

```bash
npm install

# Development (hot-reload)
./scripts/dev-linux.sh

# Production build
cargo tauri build
```

On Linux, `cpal` needs ALSA headers — `dev-linux.sh` sets the necessary env vars.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| App framework | Tauri 2 |
| Backend | Rust |
| Frontend | React 19 + TypeScript + Vite |
| Styling | Tailwind CSS |
| STT | Whisper (HTTP) |
| LLM | Ollama / llama.cpp (streaming, tool calling) |
| TTS | Piper (OpenAI-compatible) |
| Audio | cpal |
| Vector store | SQLite + Ollama embeddings |
| Agent sessions | tmux + gnome-terminal |
