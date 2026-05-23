# Phase 1: Multi-Channel-Output — Implementierungsplan

Dieser Plan deckt `show_panel`, `ask_user`, App-State-Tracking und
Sprach-Shortcuts ab. Geschätzter Aufwand: 2-3 Sessions.

## Schritt 1: show_panel Tool

### Backend (Rust)

**`src-tauri/src/state.rs`** — Neue Structs hinzufügen:

```rust
pub struct PanelInfo {
    pub title: String,
}

pub struct UiState {
    pub panel: Option<PanelInfo>,
}

// In AppState:
pub ui_state: Mutex<UiState>,
```

**`src-tauri/src/config.rs`** — ToolsConfig erweitern:

```rust
#[serde(default = "default_true")]
pub show_panel: bool,
```

Core-Prompt ergänzen um:
> "Use show_panel(title, content) to open a detail panel for tables, code,
> file listings, long output. Always still speak a short summary."

**`src-tauri/src/voice/tool_defs.rs`** — Tool-Definition:

```rust
if tools_config.show_panel {
    tools.push(serde_json::json!({
        "type": "function",
        "function": {
            "name": "show_panel",
            "description": "Open a detail panel to show formatted content that is too long or complex for speech. Use for: file listings, code, tables, diffs, build output. Still speak a short summary. Calling again replaces the panel content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Panel window title" },
                    "content": { "type": "string", "description": "Markdown-formatted content" }
                },
                "required": ["title", "content"]
            }
        }
    }));
}
```

**`src-tauri/src/pipeline.rs`** — Neuer Arm in `execute_tool()`:

```rust
"show_panel" => {
    let title = tool_call.function.arguments
        .get("title").and_then(|v| v.as_str())
        .unwrap_or("Details").to_string();
    let content = tool_call.function.arguments
        .get("content").and_then(|v| v.as_str())
        .unwrap_or("").to_string();

    // Panel-Fenster erstellen oder wiederverwenden
    if app.get_webview_window("panel").is_none() {
        let url = tauri::WebviewUrl::App("index.html?view=panel".into());
        let _ = tauri::WebviewWindowBuilder::new(app, "panel", url)
            .title(format!("Dexter — {}", title))
            .inner_size(600.0, 500.0)
            .min_inner_size(400.0, 300.0)
            .resizable(true)
            .build();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    let _ = app.emit_to("panel", "panel_content",
        serde_json::json!({ "title": title, "content": content }));

    // UI-State updaten
    let state = app.state::<AppState>();
    state.ui_state.lock().unwrap().panel = Some(PanelInfo { title: title.clone() });

    format!("Panel '{}' geöffnet.", title)
}
```

**`src-tauri/capabilities/default.json`** — `"panel"` zur windows-Liste:

```json
"windows": ["main", "settings", "panel"]
```

### Frontend

**`package.json`** — Dependencies installieren:

```bash
npm install react-markdown remark-gfm
```

**`src/App.tsx`** — Route hinzufügen:

```tsx
import { Panel } from "./panel/Panel";

// In der Render-Logik:
if (params.get("view") === "panel") return <Panel />;
if (params.get("view") === "settings") return <Settings />;
return <Orb />;
```

**`src/panel/Panel.tsx`** — Neuer Component:

```tsx
import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface PanelContent {
  title: string;
  content: string;
}

export function Panel() {
  const [panel, setPanel] = useState<PanelContent>({ title: "", content: "" });

  useEffect(() => {
    const unlisten = listen<PanelContent>("panel_content", (e) => {
      setPanel(e.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  return (
    <div className="h-screen flex flex-col bg-[#0a0a18] text-white/90 overflow-hidden">
      <div className="px-5 py-3 border-b border-white/10 text-sm font-semibold text-white/80">
        {panel.title || "Detail Panel"}
      </div>
      <div className="flex-1 overflow-y-auto px-5 py-4 prose prose-invert prose-sm max-w-none">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>
          {panel.content}
        </ReactMarkdown>
      </div>
    </div>
  );
}
```

### Testen

1. Dexter starten, Spracheingabe: "Liste alle Dateien in meinem Dev-Ordner auf"
2. Erwartung: Panel öffnet sich mit Dateiliste, Dexter sagt kurze Zusammenfassung
3. Erneut fragen → Panel-Inhalt wird ersetzt
4. Panel per X-Button schließen

---

## Schritt 2: Panel per Sprache schließen + App-State

**`src-tauri/src/pipeline.rs`** — Vor dem LLM-Call UI-Befehle abfangen:

```rust
fn handle_ui_command(app: &tauri::AppHandle, transcript: &str) -> bool {
    let text = transcript.to_lowercase();
    let close_words = ["schließ", "schliess", "close", "panel zu",
                       "fenster zu", "ok danke", "mach zu"];
    if contains_any(&text, &close_words) {
        let state = app.state::<AppState>();
        if state.ui_state.lock().unwrap().panel.is_some() {
            if let Some(w) = app.get_webview_window("panel") { let _ = w.close(); }
            state.ui_state.lock().unwrap().panel = None;
            return true; // Pipeline nicht starten
        }
    }
    false
}
```

Aufrufen am Anfang von `run_llm_pipeline()`, vor dem LLM-Request.

**`src-tauri/src/pipeline.rs`** — UI-State als System-Message injizieren:

```rust
fn build_ui_context(app: &tauri::AppHandle) -> String {
    let state = app.state::<AppState>();
    let ui = state.ui_state.lock().unwrap();
    let mut parts = Vec::new();
    if let Some(ref p) = ui.panel {
        parts.push(format!("Detail panel '{}' is open.", p.title));
    }
    if parts.is_empty() { String::new() }
    else { format!("[UI state: {}]", parts.join(" ")) }
}
```

Einfügen als System-Message vor der letzten User-Message in `all_messages`.

**`src-tauri/src/lib.rs`** — Window-Close-Event lauschen:

```rust
// Panel-Fenster-Close tracken
let app_handle = app.handle().clone();
app.listen("tauri://close-requested", move |event| {
    // UiState updaten wenn Panel geschlossen wird
});
```

### Testen

1. Panel öffnen lassen, dann sagen "Schließ das Panel" → Panel geht zu, kein LLM-Call
2. Panel öffnen, andere Frage stellen → Modell erwähnt ggf. das offene Panel

---

## Schritt 3: ask_user Tool

### Backend

**`src-tauri/src/state.rs`** — Dialog-Structs:

```rust
pub struct DialogOption {
    pub label: String,
    pub description: Option<String>,
}

pub struct DialogState {
    pub question: String,
    pub options: Vec<DialogOption>,
    pub responder: tokio::sync::oneshot::Sender<String>,
}

// In AppState:
pub pending_dialog: Mutex<Option<DialogState>>,
```

**`src-tauri/src/voice/tool_defs.rs`** — Tool-Definition mit `question` und
`options` Array (je `label` + optionales `description`).

**`src-tauri/src/pipeline.rs`** — `execute_tool`-Arm:

- Oneshot-Channel erstellen
- Sender in `AppState.pending_dialog` speichern
- `show_dialog`-Event an Frontend emitieren
- `tokio::time::timeout(60s, receiver.await)` — 60s Timeout
- Ergebnis als Tool-Result: `"User selected: {label}"`

**`src-tauri/src/pipeline.rs`** — Dialog-Interception am Anfang:

Wenn `pending_dialog` aktiv ist und neuer Transcript eingeht:
- Transcript gegen Optionen matchen (Buchstabe A-D, Zahl 1-4, deutsche
  Zahlwörter, Label-Substring)
- Bei Match: Oneshot resolven, Pipeline returnt ohne LLM-Call
- Kein Match: Normal an LLM weiterleiten

**`src-tauri/src/commands.rs`** — `resolve_dialog(selected: String)` Command
für Click-Responses aus dem Frontend.

### Frontend

**`src/orb/Orb.tsx`** — Dialog-Overlay:

```tsx
const [dialog, setDialog] = useState<DialogPayload | null>(null);

useEffect(() => {
  const un1 = listen("show_dialog", (e) => setDialog(e.payload));
  const un2 = listen("dismiss_dialog", () => setDialog(null));
  return () => { un1.then(f => f()); un2.then(f => f()); };
}, []);
```

Rendering: Overlay über den Bubbles mit Frage-Text + Buttons (A/B/C/D-Stil).
Klick ruft `invoke("resolve_dialog", { selected: label })` auf.

### Testen

1. Mehrdeutige Frage provozieren (oder System-Prompt so schreiben, dass
   das Modell bei Unklarheiten `ask_user` nutzt)
2. Dialog erscheint → per Klick "B" wählen → Modell arbeitet weiter
3. Dialog erscheint → per Sprache "B" sagen → selbes Ergebnis
4. Timeout testen: 60s warten → Dialog verschwindet

---

## Reihenfolge

1. **show_panel** (1 Session) — sofort nützlich, einfachster Einstieg
2. **Panel-Close + App-State** (halbe Session) — aufbauend auf 1
3. **ask_user** (1-2 Sessions) — komplexer wegen Oneshot + Voice-Resolution

## Dependencies

- `react-markdown` + `remark-gfm` (npm)
- Tailwind `@tailwindcss/typography` Plugin für `prose`-Klassen (optional,
  alternativ eigene Markdown-Styles)
