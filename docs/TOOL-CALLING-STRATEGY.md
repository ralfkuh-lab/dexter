# Tool-Calling-Optimierung für kleine LLMs

## Problem

Gemma 4 E4B (und ähnliche 4-8B Modelle) rufen Tools oft nicht korrekt auf:
- Schreiben `call switch_mode(mode: "agy_session")` als Text statt echten Tool-Call
- Beschreiben was sie tun würden ("Ich zeige dir das im Panel") ohne es zu tun
- Setzen Tool-Argumente falsch (Shell-Command statt Ergebnis in show_panel)

## Strategie

Drei Hebel, in dieser Reihenfolge angehen:

### 1. System-Prompt iterativ verbessern

Die Datei `system-prompt.md` im Projekt-Root enthält den System-Prompt.
Änderungen werden beim App-Neustart automatisch geladen.

**Test-Methodik:**
```bash
# Test-Harness starten (sendet Szenarien direkt an llama.cpp API)
python3 tests/tool_calling/eval.py

# Mit alternativem Prompt testen
python3 tests/tool_calling/eval.py --system-prompt-file prompts/v2.md

# Mit anderem Modell testen (z.B. qwen3.5-4b)
python3 tests/tool_calling/eval.py --model qwen3.5-4b
```

**Szenarien** in `tests/tool_calling/scenarios.json` — jedes definiert:
- `prompt`: User-Eingabe
- `expected_tool`: erwarteter Tool-Name (oder `null` für reine Textantwort)
- `expected_args`: optionale Argument-Validierung

**Bewertung:**
- `PASS`: Tool-Call mit korrektem Namen + korrekten Args
- `WRONG_TOOL`: Tool-Call, aber falsches Tool
- `TEXT_INSTEAD`: Kein Tool-Call, obwohl einer erwartet
- `UNWANTED_TOOL`: Tool-Call, obwohl keiner erwartet
- Score = PASS / Gesamtzahl

### 2. Rescue Parsing (Backend)

Wenn das LLM Text statt Tool-Call liefert, automatisch versuchen den
Tool-Call aus dem Text zu extrahieren. Zu implementieren in
`src-tauri/src/voice/llm/mod.rs`:

Patterns zum Erkennen:
- `call tool_name(key: "value", ...)` → häufigstes Fehlformat
- `{"tool": "name", "args": {...}}` → JSON-Format
- `tool_name({"key": "value"})` → Funktions-Syntax
- XML `<tool_call>` → bereits implementiert

### 3. Retry Nudging (Backend)

Wenn Rescue auch fehlschlägt: eine Retry-Nachricht einfügen und nochmal
ans LLM senden. Max 1 Retry pro Turn.

Nudge-Text:
```
"Your previous response was plain text, but this task requires a tool call. 
Please respond with an actual tool call, not a description of what you would do."
```

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
    → openai.rs oder ollama.rs (je nach llm_provider)
    → Request an LLM mit tools[] im OpenAI-Format
    → Response: StreamResult::Content oder StreamResult::ToolCalls
  → pipeline.rs: Tool-Call-Schleife (max 5 Runden)
    → tool_executor.rs: execute_tool()
    → Ergebnis zurück ans LLM
```

Tool-Definitionen: `src-tauri/src/voice/tool_defs.rs`
System-Prompt: `system-prompt.md` (geladen von `config.rs::core_system_prompt()`)
LLM-Provider: `llm_provider` in Config (`"openai"` = llama.cpp, `"ollama"` = Ollama)

## Modell-Alternativen zum Testen

| Modell | Pfad in LM Studio | Erwartung |
|--------|-------------------|-----------|
| Gemma 4 E4B IQ4_XS | aktuell geladen | Baseline |
| Qwen 3.5 4B Q4_K_M | `lmstudio-community/qwen3.5-4b` | Qwen hat generell besseres Tool-Calling |
| Qwen 3.5 2B | `lmstudio-community/qwen3.5-2b` | Schneller, aber weniger fähig |

Modell wechseln: In Dexter Settings oder direkt im llama.cpp Docker-Container.
