# Tool-Calling-Optimierung für Gemma 4 E4B

## Problem

Gemma 4 E4B ruft Tools oft nicht korrekt auf:
- Schreibt `call switch_mode(mode: "agy_session")` als Text statt echten Tool-Call
- Beschreibt was es tun würde ("Ich zeige dir das im Panel") ohne es zu tun
- Setzt Tool-Argumente falsch (Shell-Command statt Ergebnis in show_panel)
- Erkennt STT-Verhörer nicht (z.B. "AGI" statt "agy", "Cloud" statt "Claude")

## Aktuelle Konfiguration

- **Modell:** Gemma 4 E4B IQ4_XS auf llama.cpp (Port 8081)
- **System-Prompt:** `system-prompt.md` im Projekt-Root (wird beim App-Start geladen)
- **Tool-Format:** OpenAI Function Calling via `/v1/chat/completions`
- **Modellwechsel ist aufwändig** (Server-Stack neu laden) — erstmal nur Gemma 4 E4B optimieren

## Strategie

### Schritt 1: System-Prompt iterativ verbessern

**Workflow:**
1. Baseline messen: `python3 tests/tool_calling/eval.py --verbose`
2. `system-prompt.md` editieren
3. Eval erneut laufen lassen (kein App-Neustart nötig — eval sendet direkt an API)
4. Wiederholen bis Score stabil ≥80%

```bash
# Vollständiger Eval-Lauf
python3 tests/tool_calling/eval.py --verbose

# Einzelnes Szenario testen
python3 tests/tool_calling/eval.py -s stt_agy_agi --verbose

# Mit alternativem Prompt testen
python3 tests/tool_calling/eval.py --system-prompt-file prompts/v2.md
```

### Schritt 2: Rescue Parsing (Backend)

Wenn das LLM Text statt Tool-Call liefert, automatisch versuchen den
Tool-Call aus dem Text zu extrahieren. Zu implementieren in
`src-tauri/src/voice/llm/mod.rs`:

Patterns zum Erkennen:
- `call tool_name(key: "value", ...)` → häufigstes Fehlformat bei Gemma
- `{"tool": "name", "args": {...}}` → JSON-Format
- `tool_name({"key": "value"})` → Funktions-Syntax
- XML `<tool_call>` → bereits implementiert

### Schritt 3: Retry Nudging (Backend)

Wenn Rescue auch fehlschlägt: eine Retry-Nachricht einfügen und nochmal
ans LLM senden. Max 1 Retry pro Turn.

## Szenarien-Kategorien

Die Testszenarien (`tests/tool_calling/scenarios.json`) decken ab:

### Basis-Tool-Calls (muss funktionieren)
- Zeitfragen → `get_current_time`
- Clipboard → `read_clipboard`
- Screenshot → `take_screenshot`
- Shell-Befehle → `run_command`
- URLs und Websuche → `open_url`, `web_search`, `web_fetch`
- Apps → `list_running_apps`
- Modus-Wechsel → `switch_mode`

### STT-Fehlertoleranz (kritisch!)
- **Agent-Namen verhört:** "AGI"→agy, "Cloud"→Claude, "Codecks"→Codex, "Antigravity"→agy
- **Pfade gesprochen:** "home dev dexter" → ~/dev/dexter
- **URLs gesprochen:** "Heise Punkt DE" → heise.de
- **Füllwörter:** "Ähm ja also wie spät ist es jetzt?"
- **Gebrochene Sätze:** "Die, also die Zwischenablage, was steht da drin?"
- **Umgangssprachlich:** "Was hab ich denn da kopiert", "Guck mal was auf dem Screen ist"

### Ambiguität (sollte nachfragen via ask_user)
- **Fehlender Agent:** "Mach mal ne Coding Session auf" (welcher Agent?)
- **Zu vage:** "Kannst du das mal machen?" (was genau?)

### Negative Szenarien (soll KEIN Tool aufrufen)
- Allgemeinwissen: "Was ist die Hauptstadt von Frankreich?"
- Begrüßung: "Hallo Dexter!"
- Meinungsfragen: "Was hältst du von Rust?"

## Bewertung

| Status | Bedeutung |
|--------|-----------|
| `PASS` | Tool-Call mit korrektem Namen + korrekten Args |
| `WRONG_TOOL` | Tool-Call, aber falsches Tool |
| `WRONG_ARGS` | Richtiges Tool, aber falsche Argumente |
| `TEXT_INSTEAD` | Kein Tool-Call, obwohl einer erwartet |
| `UNWANTED_TOOL` | Tool-Call, obwohl keiner erwartet |
| `ERROR` | API-Fehler |

**Ziel:** ≥80% PASS-Rate (aktuell vermutlich 30-50%)

## Referenz: Forge Framework

Das Forge-Projekt (`/home/ralf/dev/forge`) hat genau dieses Problem für
8B-Modelle gelöst (5% → 84% Accuracy). Relevante Dateien:

| Technik | Forge-Datei |
|---------|-------------|
| Rescue Parsing | `src/forge/prompts/templates.py` → `rescue_tool_call()` |
| Nudge-System | `src/forge/prompts/nudges.py` |
| Response Validation | `src/forge/guardrails/response_validator.py` |
| Synthetic Respond Tool | `src/forge/tools/respond.py` |
| Client-Adapter (llama.cpp) | `src/forge/clients/llamafile.py` |

**Wichtigste Forge-Technik: Synthetic Respond Tool**

Forge zwingt das LLM IMMER ein Tool aufzurufen — auch für reine Text-
antworten (über ein `respond(message: "...")` Tool). Das eliminiert die
Entscheidung "Tool oder Text?" und erhöht die Accuracy massiv. Kann als
letzter Hebel eingesetzt werden, wenn Prompt + Rescue + Retry nicht reichen.

## Dexter-Architektur für Tool-Calls

```
User-Text
  → pipeline.rs: run_llm_pipeline()
  → voice/llm/mod.rs: chat_streaming()
    → openai.rs (llm_provider="openai" für llama.cpp)
    → POST /v1/chat/completions mit tools[] im OpenAI-Format
    → Response: StreamResult::Content oder StreamResult::ToolCalls
  → pipeline.rs: Tool-Call-Schleife (max 5 Runden)
    → tool_executor.rs: execute_tool()
    → Ergebnis zurück ans LLM
```

| Datei | Rolle |
|-------|-------|
| `system-prompt.md` | System-Prompt (editierbar, geladen von config.rs) |
| `src-tauri/src/voice/tool_defs.rs` | Tool-Definitionen im OpenAI-Format |
| `src-tauri/src/voice/llm/openai.rs` | API-Client für llama.cpp |
| `src-tauri/src/voice/llm/mod.rs` | Dispatch + XML-Parsing |
| `src-tauri/src/pipeline.rs` | Tool-Call-Schleife + TTS-Streaming |
| `src-tauri/src/tool_executor.rs` | Tool-Ausführung |
| `src-tauri/src/config.rs` | Config + Prompt-Loading |
