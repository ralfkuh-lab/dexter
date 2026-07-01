# Code-Review Dexter

**Datum:** 2026-07-01
**Umfang:** Rust-Backend (`src-tauri/src/`, ~6.400 Zeilen), Frontend
(`src/`, ~1.800 Zeilen TS/TSX), Test-Harness (Python), Scripts, Konfiguration.
**Methode:** Statische Code-Analyse. Keine Builds/Tests ausgeführt. Alle unten
gelisteten Findings wurden gegen den tatsächlichen Quellcode gegengeprüft
(Zeilenverweise verifiziert).

Die breite Datei-Durchsicht wurde an Gemini (agy) delegiert, die einzelnen
Findings anschließend am Code verifiziert und bewertet. Findings, die sich bei
der Verifikation als falsch oder ungenau erwiesen, sind entsprechend markiert.

> **Hinweis zur veralteten Modul-Doku:** Die Modul-Übersicht in `CLAUDE.md`
> ist nicht mehr aktuell. `voice.rs` ist inzwischen ein Verzeichnis `voice/`
> (`mod.rs`, `stt.rs`, `tts.rs`, `audio.rs`, `tool_defs.rs`, `llm/`), und es
> gibt neue Top-Level-Module `commands.rs`, `hands_free.rs`, `agent_draft.rs`,
> `main.rs`. Die Liste sollte nachgezogen werden — sie ist der erste
> Anlaufpunkt für jede neue Session.

---

## Zusammenfassung nach Schweregrad

| # | Schweregrad | Bereich | Kurzbeschreibung | Status |
|---|-------------|---------|------------------|--------|
| 1 | **Kritisch** | Sicherheit | Automation-API ohne Auth → RCE-Pfad über Browser (DNS-Rebinding/CSRF) | ✅ behoben |
| 2 | **Hoch** | Sicherheit | `ingest_file` liest beliebige absolute Pfade (Path-Traversal / Daten-Exfil via RAG) | ✅ behoben |
| 3 | **Hoch** | Sicherheit | Sandbox-Blocklist per Env-Var-Präfix umgehbar (`FOO=1 sudo …`) | ✅ behoben |
| 4 | **Hoch** | Korrektheit | Ollama-Provider ignoriert `forced_tool` → Agent-Draft unzuverlässig | offen (Design-Entscheidung) |
| 5 | **Mittel** | Robustheit | Setup-Panik bei belegtem Hotkey → App startet nicht |
| 6 | **Mittel** | Korrektheit | Audio-Blob-Leak bei Unterbrechung der Sprachausgabe |
| 7 | **Mittel** | Korrektheit | Kein Debounce → IPC-/Netzwerk-Flut bei Tastatureingabe (ModelSelect u.a.) |
| 8 | **Mittel** | Robustheit | Laufendes Tool (`ask_user`, 60s) nicht durch Cancel-Token abbrechbar |
| 9 | **Niedrig** | Robustheit | Blockierendes `std::thread::sleep`/`fs::read_to_string` im async/IPC-Kontext |
| 10 | **Niedrig** | UX | Blockierendes `prompt()` statt nativem Datei-Dialog im KnowledgeTab |
| 11 | **Niedrig** | Robustheit | Fehlende `.catch()` bei Settings-Init und Test-JSON-Parsing |

> **Update 2026-07-01:** Findings 1–3 (Sicherheit) wurden behoben — Details am
> Ende dieses Dokuments unter *Behebungen*. `cargo check` + `cargo test sandbox`
> laufen grün.

---

## Kritisch

### 1. Automation-API ohne Authentifizierung — RCE-Pfad über den Browser

**Datei:** `src-tauri/src/automation.rs:156-188`

Die Automation-API bindet auf `127.0.0.1:9877` ohne Token, ohne
`Host`-Header-Prüfung und ohne CORS-Restriktion. `POST /text` ruft
`commands::submit_text` auf; befindet sich Dexter in einem Session-Modus
(`CodexSession`/`ClaudeSession`/…), wird der Text über
`agent_session::send_keys` per `tmux send-keys` **direkt in den laufenden
CLI-Agenten** geschrieben (verifiziert: `pipeline.rs:993`, erreichbar aus
`submit_text` wenn `app_mode != Chat`).

**Fehlerszenario:** Der Nutzer hat eine Claude-/Codex-Session offen und
besucht nebenbei eine beliebige Webseite. Deren JavaScript sendet einen
simplen `POST http://127.0.0.1:9877/text` (bzw. per DNS-Rebinding gegen den
127.0.0.1-Bind). `/text` ist ein `application/json`-POST ohne
Preflight-erzwingende Header → landet als Prompt im CLI-Agenten, der ihn als
Coding-Anweisung ausführt. Ergebnis: **Remote Code Execution**. `POST /quit`
beendet die App, `POST /ptt/press` steuert das Mikrofon.

**Empfehlung:**
- Beim App-Start ein zufälliges Token generieren, ins Frontend injizieren und
  bei jedem Request in einem Custom-Header (`X-Dexter-Token`) verlangen.
- Zusätzlich `Host`-Header explizit gegen `127.0.0.1:9877`/`localhost:9877`
  prüfen (Schutz gegen DNS-Rebinding).
- Alternativ/ergänzend: die API nur aktivieren, wenn eine Env-Variable gesetzt
  ist (z. B. `DEXTER_AUTOMATION=1`), damit sie im Normalbetrieb gar nicht
  lauscht. Für einen reinen E2E-Test-Endpunkt ist das die einfachste
  Härtung — der Angriffsvektor entfällt in der ausgelieferten App komplett.

---

## Hoch

### 2. `ingest_file` liest beliebige absolute Pfade

**Datei:** `src-tauri/src/commands.rs:287-288`

```rust
pub async fn ingest_file(app: tauri::AppHandle, path: String) -> Result<usize, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Read failed: {}", e))?;
```

Kein Pfad-Check. Ein kompromittiertes/manipuliertes Frontend (oder — in
Kombination mit Finding 1 — ein manipulierter IPC-Aufruf) kann `/etc/passwd`,
`~/.ssh/id_rsa`, `~/.config/voice-assistant/config.json` etc. einlesen. Der
Inhalt landet in der RAG-SQLite und ist danach über gezielte LLM-Abfragen
abrufbar (Daten-Exfiltration).

**Empfehlung:** Pfad kanonisieren (`std::fs::canonicalize`) und gegen die in
den Sandbox-Settings konfigurierte `readable_paths`-Whitelist bzw. gegen ein
festes Ingest-Verzeichnis prüfen, bevor gelesen wird. Symlinks nach der
Kanonisierung erneut prüfen.

### 3. Sandbox-Blocklist per Env-Var-Präfix umgehbar

**Datei:** `src-tauri/src/sandbox.rs:194-224`

`validate_command` bestimmt das „erste Wort" via `split_whitespace().next()`
und vergleicht dessen Basename gegen `BLOCKED_COMMANDS` (`sudo`, `su`, …).
Bei `FOO=1 sudo apt-get …` ist das erste Wort aber `FOO=1` — der Check greift
nicht. Auch die Pipeline-Segmentierung (`split('|')`/`"&&"`/`';'`) hilft nicht,
da jedes Segment wieder mit demselben „erstes-Wort"-Verfahren geprüft wird.

**Einordnung:** Der Guarded-Modus ist ohnehin keine echte
Filesystem-Sandbox (siehe Finding unten in *Weitere Beobachtungen*), sondern
Defense-in-Depth. Trotzdem ist eine Blocklist, die den simpelsten Bypass nicht
abfängt, irreführend — sie suggeriert Schutz, den es nicht gibt.

**Empfehlung:** Führende `KEY=VALUE`-Zuweisungen vor der Prüfung überspringen
**und** alle Tokens jedes Segments (nicht nur das erste) gegen die Blocklist
prüfen. Grundsätzlich ist eine Blocklist gegen Shell-Kreativität kaum
gewinnbar — die Kernaussage „nur Docker-Modus isoliert wirklich" gehört
prominent in Doku und UI.

### 4. Ollama-Provider ignoriert `forced_tool`

**Dateien:** `src-tauri/src/voice/llm/mod.rs:82-94`, `voice/llm/ollama.rs:53-58`

```rust
// mod.rs
if config.llm_provider == "ollama" {
    return ollama::chat_streaming(app, config, messages, tools, sentence_tx).await;
}
openai::chat_streaming(app, config, messages, tools, forced_tool, sentence_tx).await
```

Der Ollama-Pfad nimmt `forced_tool` nicht einmal als Parameter entgegen — das
Argument wird in `mod.rs` stillschweigend verworfen. `agent_draft.rs:169` ruft
`chat_streaming` aber mit erzwungenem `update_agent_draft` auf. Bei
Ollama-Konfiguration wird das Tool-Forcing also ignoriert; kleine Modelle
produzieren dann oft Freitext statt des Tool-Calls, und das Hands-free-
Prompt-Editing wird unzuverlässig (Draft aktualisiert sich nicht).

**Empfehlung:** In `ollama.rs` das erzwungene Tool über Ollamas
Format-/Tool-Choice-Mechanismus umsetzen (bzw. per JSON-Schema im
`format`-Feld). Mindestens: den ignorierten Parameter sichtbar machen (nicht
still schlucken), damit der Bruch nicht unbemerkt bleibt.

---

## Mittel

### 5. Setup-Panik bei belegtem Hotkey

**Datei:** `src-tauri/src/lib.rs:157,165` (in Kombination mit `:266-267`)

Die Setup-Closure nutzt `?` bei `register_ptt_shortcut` und
`register_dictation_shortcut`. Schlägt die Registrierung fehl (z. B. F9/F10
bereits von einer anderen Desktop-App belegt), bricht das Setup ab, und
`.run(...).expect("error while running tauri application")` paniert → die App
startet gar nicht.

**Empfehlung:** Registrierungsfehler abfangen, loggen und dem Nutzer als
Toast/Settings-Warnung anzeigen, statt den Start abzubrechen. Ein
Sprachassistent ohne PTT-Hotkey ist unschön, aber besser als eine App, die
sich nicht öffnen lässt.

### 6. Audio-Blob-Leak bei Unterbrechung der Sprachausgabe

**Datei:** `src/orb/Orb.tsx:143-158`

`stopAllAudio()` pausiert das aktuell spielende Element und gibt die noch in
der Queue liegenden Blob-URLs frei (`:151`) — die URL des **gerade spielenden**
Elements (`currentAudioRef.current.src`, per `URL.createObjectURL` bei `:258`
erzeugt) wird jedoch nie `revokeObjectURL`'d. Jede vorzeitige Unterbrechung
(Nutzer drückt PTT / spricht dazwischen) leakt so einen Audio-Blob im
Webview-Prozess. Die `onended`/`onerror`-Pfade (`:184-185`) machen es korrekt —
nur der Stop-Pfad nicht.

**Empfehlung:** In `stopAllAudio()` vor dem Nullen von `currentAudioRef.current`
die aktuelle `src` freigeben, falls sie mit `blob:` beginnt:
```typescript
if (currentAudioRef.current) {
  currentAudioRef.current.pause();
  const src = currentAudioRef.current.src;
  if (src.startsWith("blob:")) URL.revokeObjectURL(src);
  currentAudioRef.current.onended = null;
  currentAudioRef.current.onerror = null;
  currentAudioRef.current = null;
}
```

### 7. Kein Debounce → IPC-/Netzwerk-Flut bei Tastatureingabe

**Dateien:** `src/components/ModelSelect.tsx:30`, `src/agent-draft/AgentDraft.tsx`,
`src/orb/DictationBuffer.tsx`

`ModelSelect` triggert `list_models` in einem `useEffect` mit
`[baseUrl, provider]` als Dependencies. Tippt der Nutzer im ConfigTab eine
LLM-Base-URL, feuert bei **jedem Tastenanschlag** ein `invoke("list_models")`,
das im Backend einen HTTP-Request gegen die (noch unvollständige, ungültige)
URL absetzt. Analog schicken die Editoren `update_agent_draft` /
`update_dictation_buffer` pro Keystroke ans Backend.

**Empfehlung:** React-State sofort aktualisieren, aber das `invoke`
debouncen (250–500 ms) bzw. bei `ModelSelect` erst `onBlur` laden. Reduziert
Serialisierungs-Overhead, verhindert Requests gegen halbfertige URLs.

### 8. Laufendes Tool nicht durch Cancel-Token abbrechbar

**Datei:** `src-tauri/src/pipeline.rs:800,839` + `tool_executor.rs:11`

Präzisierung gegenüber dem Erstbefund: Der zweite Tool-Loop prüft
`cancel_llm.is_cancelled()` **zwischen** den Tool-Calls (`:826`), der erste
Loop (`:800`) tut das nicht. Vor allem aber nimmt `execute_tool` **selbst**
keinen Cancel-Token entgegen — ein bereits laufender, blockierender Tool-Call
(z. B. `ask_user` mit bis zu 60 s Wartezeit) läuft nach einem
Pipeline-Abbruch im Hintergrund weiter. Startet der Nutzer inzwischen eine
neue Anfrage, können sich Effekte (Audio, RAG-Writes) überlagern.

**Empfehlung:** `CancellationToken` in `execute_tool` durchreichen und den
Aufruf in `pipeline.rs` in `tokio::select!` gegen `cancel_llm.cancelled()`
setzen, sodass ein laufendes Tool sauber abgebrochen wird. Zusätzlich den
`is_cancelled()`-Check auch in den ersten Tool-Loop aufnehmen.

---

## Niedrig

### 9. Blockierende Aufrufe im async/IPC-Kontext

**Datei:** `src-tauri/src/commands.rs:446` (`std::thread::sleep(100ms)` in
`stop_recording_and_process`), `commands.rs:288` (`std::fs::read_to_string`
in `async fn ingest_file`).

Der synchrone Sleep blockiert einen Tauri-IPC-Threadpool-Thread; das
synchrone Datei-Lesen blockiert den Async-Executor. Bei kleinen Dateien
unkritisch, bei großen Ingest-Dateien spürbar.

**Empfehlung:** In `async`-Kontexten `tokio::time::sleep` bzw.
`tokio::fs::read_to_string` verwenden (oder `spawn_blocking`). `stop_recording`
ist aktuell `pub fn` (sync) — hier ggf. auf `async fn` umstellen oder das
Warten in den Recording-Thread verlagern.

### 10. Blockierendes `prompt()` statt nativem Datei-Dialog

**Datei:** `src/settings/KnowledgeTab.tsx:43`

`const path = prompt("Enter file path:")` blockiert den Render-Thread und
zwingt den Nutzer, absolute Pfade manuell einzutippen (fehleranfällig, und
Angriffsfläche für Finding 2).

**Empfehlung:** `@tauri-apps/plugin-dialog` (nativer, asynchroner
Datei-Auswahldialog). Plugin muss in `capabilities`/`lib.rs` registriert sein.

### 11. Fehlende Fehlerbehandlung bei Settings-Init und Test-JSON

**Dateien:** `src/settings/Settings.tsx:16-20`, `tests/tool_calling/eval.py`
(JSON-Parsing der LLM-Antwort).

Die drei `invoke(...).then(...)` im Settings-`useEffect` haben kein `.catch()`
— schlägt ein Command fehl, bleibt das Menü dauerhaft leer, ohne Fehlerhinweis.
Im Test-Harness wirft `json.loads(...)` bei einer nicht-JSON-Antwort (leerer
Body, HTML-Fehlerseite) einen unbehandelten `JSONDecodeError` → die Suite
crasht statt den Fall als Testfehlschlag zu werten.

**Empfehlung:** `.catch()` mit Fehlerzustand/Toast im Settings-Effekt; im
Test-Runner `json.JSONDecodeError` abfangen und als Fail protokollieren.

---

## Weitere Beobachtungen (kein akutes Risiko)

- **Guarded-Sandbox ist keine Filesystem-Isolation** (`sandbox.rs:291`): Im
  Default-Modus laufen Befehle via `sh -c` direkt auf dem Host im Workspace;
  `readable_paths` wird nur im Docker-Modus als Mount-Whitelist genutzt. Das
  ist so gewollt, sollte aber in der UI/Doku unmissverständlich stehen, damit
  niemand Guarded für eine echte Sandbox hält. Positiv: Env-Sanitisierung
  (`STRIPPED_ENV_VARS`), PATH-Override und Timeout-Kill sind sauber gelöst.
- **`send_keys` ist gegen Shell-Injection abgesichert** (`agent_session.rs:116`):
  nutzt `tmux send-keys -l` mit Args-Array (kein `sh -c`). Gut. Das Risiko
  liegt nicht hier, sondern in der ungeschützten API davor (Finding 1).
- **Piper-TTS-Server-Threadsafety** (`scripts/piper-openai-server.py`): Der
  synchrone FastAPI-Endpunkt lässt parallele `synthesize_wav`-Aufrufe auf
  dieselbe (nicht thread-sichere) PiperVoice/ONNX-Instanz zu. Liegt im
  separaten Stack-Kontext, aber ein `threading.Lock` bzw. `async def` wäre
  eine billige Absicherung.
- **System-Prompt-Header-Duplikation** (`system-prompt.md`): Von agy gemeldet,
  hier **nicht bestätigt** — bitte gesondert prüfen; der eval.py-Parser splittet
  nur am ersten `---`, was bei doppeltem Header den zweiten Block im Prompt
  belassen würde.

---

## Empfohlene Reihenfolge

1. **Finding 1** (Automation-API absichern) — höchste Priorität, echter
   RCE-Pfad. Schnellste Härtung: API hinter Env-Flag legen.
2. **Finding 2 + 3** (Pfad-Check in `ingest_file`, Sandbox-Bypass) —
   Sicherheits-Basics.
3. **Finding 4** (Ollama `forced_tool`) — bricht ein Kern-Feature bei
   Ollama-Nutzung.
4. **Finding 5, 6** (Setup-Panik, Audio-Leak) — Stabilität im Alltag.
5. Rest nach Kapazität.

Positiv hervorzuheben: Timeout-/Kill-Logik der Sandbox, Multibyte-sichere
Truncation (mit Test), saubere `send-keys`-Absicherung und die insgesamt klar
modularisierte Backend-Struktur.

---

## Behebungen (2026-07-01)

Findings 1–3 wurden umgesetzt (Implementierung via Codex CLI, Review + Verifikation
durch Claude). Verifiziert mit `cargo check` (kompiliert) und `cargo test sandbox`
(3/3 grün, inkl. neuem Env-Bypass-Test).

**Finding 1 — Automation-API** (`src-tauri/src/automation.rs`, `scripts/`)
- API startet nur noch, wenn `DEXTER_AUTOMATION=1` gesetzt ist — im
  ausgelieferten/normalen Betrieb lauscht sie gar nicht (Angriffsvektor entfällt).
- `Host`-Header-Middleware (`require_local_host`) lehnt Requests ab, deren Host
  nicht exakt `127.0.0.1:9877`/`localhost:9877` ist (Schutz gegen DNS-Rebinding).
- `scripts/dev-linux.sh` und `scripts/run-linux.sh` setzen `DEXTER_AUTOMATION=1`,
  damit die lokalen Python-E2E-Tests weiterlaufen.

**Finding 2 — `ingest_file`** (`src-tauri/src/commands.rs`)
- Pfad wird via `std::fs::canonicalize` aufgelöst (Symlinks/`..`) und muss unter
  einem der `config.sandbox.readable_paths` liegen, sonst „Zugriff verweigert".
- **Verhaltensänderung:** Nur noch Dateien unter den konfigurierten
  `readable_paths` (Default: `~/Documents`, `~/Desktop`, `~/Downloads`,
  `~/Projects`) sind ingestierbar. Für andere Verzeichnisse muss die Whitelist
  in den Sandbox-Settings erweitert werden.

**Finding 3 — Sandbox-Blocklist** (`src-tauri/src/sandbox.rs`)
- Neue Hilfsfunktion `first_command_token` überspringt führende
  `KEY=VALUE`-Env-Zuweisungen, bevor gegen `BLOCKED_COMMANDS` geprüft wird — an
  beiden Prüfstellen (erstes Wort + Pipeline-Segmente). Regressionstest ergänzt.
