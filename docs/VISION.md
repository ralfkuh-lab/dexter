# Dexter — Vision: Voice-First Desktop Control

## Das Endziel

Dexter wird eine vollständige Voice-First-Schnittstelle zum Rechner. Der Nutzer
kann auf dem Sofa liegen und per Sprache seinen Computer bedienen — inklusive
kompletter Coding-Sessions, Dateimanagement, Web-Recherche und System-
Administration. Keine Tastatur, keine Maus nötig.

Das ist kein Chatbot. Das ist eine Stimme, die den Rechner versteht und bedient.

## Architektur-Säulen

### 1. Multi-Channel-Output

Das Modell hat drei Ausgabekanäle:

- **Sprache** — kurze, gesprochene Zusammenfassungen und Dialoge. Wie bisher,
  aber bewusst als ein Kanal unter mehreren.
- **Detail-Panel** — separates Fenster mit Markdown-gerendertem Inhalt: Code,
  Tabellen, Dateilisten, Diffs, Build-Output. Für alles, was zu lang oder zu
  komplex für Sprache ist.
- **Interaktive Dialoge** — Rückfragen mit Auswahloptionen (A/B/C/D), die per
  Klick oder Sprache beantwortet werden können.

Realisierung: Tool-basiert (`show_panel`, `ask_user`), nicht über strukturierte
JSON/XML-Formate. Das Modell ruft Tools auf wie bei jedem anderen Werkzeug.

### 2. App-State-Awareness

Das Modell weiß, was auf dem Bildschirm passiert:

- Welche Panels/Dialoge sind offen
- Welche Dateien wurden zuletzt bearbeitet
- Welcher Build-Status aktiv ist
- Was der User zuletzt per Screenshot gesehen hat

Der UI-State wird als kontextuelle System-Message vor dem letzten User-Turn
injiziert, nicht im System-Prompt (um den Prompt-Cache nicht zu brechen).

### 3. Voice-First Interaction Patterns

Spracheingabe ist fehlerbehaftet und langsam im Vergleich zur Tastatur. Dexter
kompensiert das durch:

- **STT-Toleranz** — Pfade, Namen und Befehle werden fuzzy interpretiert.
  "Home Dev" → `~/dev`, "Klammer auf" → `(`.
- **Proaktives Handeln** — Statt Rückfragen zu stellen, sucht Dexter selbst nach
  plausiblen Interpretationen (z.B. `ls` ausführen wenn ein Pfad unklar ist).
- **Sprach-Shortcuts** — "OK", "Schließ das", "A", "Weiter" werden vor dem
  LLM-Call abgefangen und direkt ausgeführt (spart ~1s Latenz).
- **Kontextfortsetzung** — "Und jetzt?", "Mach weiter", "Das gleiche nochmal"
  werden vom Modell im Kontext der laufenden Session verstanden.
- **Bestätigungs-Dialoge** — Für destruktive Aktionen (Dateien löschen, Force-
  Push) fragt Dexter per `ask_user` nach statt blind auszuführen.

### 4. Voice Coding Workflow

Das ambitionierteste Feature: komplette Coding-Sessions per Sprache.

**Code bearbeiten:**
- "Öffne die Datei config.rs" → Panel zeigt den Inhalt
- "Ändere die Funktion default_prompt, ersetze den Rückgabewert durch ..."
- "Füge nach Zeile 42 ein: ..." → Dexter führt die Änderung aus, zeigt Diff
- "Mach das rückgängig" → Git-basiertes Undo

**Code reviewen:**
- "Zeig mir den Diff seit dem letzten Commit" → Panel mit farbigem Diff
- "Was hat sich in voice.rs geändert?" → gesprochene Zusammenfassung + Panel
- "Sieht gut aus, committe das mit der Message 'Fix XY'"

**Build und Test:**
- "Kompilier das mal" → `cargo check`, Fehler gesprochen + im Panel
- "Lauf die Tests" → Ergebnis-Zusammenfassung gesprochen, Details im Panel
- "Der Test in Zeile 87 schlägt fehl, zeig mir warum" → Kontext-Analyse

**Navigation:**
- "Welche Dateien gibt es im src-Verzeichnis?" → Panel mit Baumstruktur
- "Zeig mir die Funktion handle_ptt_release" → Panel springt zur Stelle
- "Wo wird AppState verwendet?" → grep-Ergebnis im Panel

**Cloud-Delegation:**
- Für komplexe Code-Generierung oder Refactoring kann Dexter an einen
  Cloud-Agenten delegieren (Claude API, Codex) und das Ergebnis lokal
  anwenden. Das lokale 4B-Modell orchestriert, die Cloud-Modelle liefern.

### 5. Desktop-Steuerung

Über das Coding hinaus:

- **App-Steuerung** — "Öffne Firefox", "Schließ das Terminal", "Wechsel zu VS Code"
- **System-Info** — "Wie viel Speicher ist noch frei?", "Welche Prozesse fressen CPU?"
- **Dateisystem** — "Verschieb die Datei nach Downloads", "Lösch die alten Logs"
- **Web** — "Such mal nach Rust async patterns", "Was steht auf der Seite?"
- **Benachrichtigungen** — "Erinner mich in 30 Minuten an den Build"

### 6. Sicherheit

Voice-gesteuerte Shell-Befehle sind mächtig und gefährlich:

- **Sandbox bleibt Pflicht** — `run_command` läuft weiterhin in der konfigurierten
  Sandbox (Guarded/Docker). Destruktive Befehle werden geblockt.
- **Bestätigung für Kritisches** — Dateilöschung, Git-Push, Systemänderungen
  erfordern explizite Bestätigung per `ask_user`.
- **Audit-Log** — Alle ausgeführten Befehle werden protokolliert.
- **Kein Netzwerk-Default** — Shell-Befehle haben standardmäßig keinen
  Netzwerkzugang (konfigurierbar).

## Hardware-Realität

- **LLM:** Gemma 4 E4B (4B Parameter), lokal auf RTX 3060 (6 GB VRAM),
  ~60-100 tok/s. Für Orchestrierung und einfache Aufgaben ausreichend.
- **Cloud-Fallback:** Für komplexe Code-Generierung, große Kontexte oder
  anspruchsvolle Reasoning-Aufgaben an Claude/Codex delegieren.
- **STT:** Whisper large-v3-turbo, lokal auf CUDA, ~1s Latenz.
- **TTS:** Piper (CPU, Deutsch), ~50ms pro Satz.
- **Display:** Laptop-Bildschirm oder TV als zweiter Monitor für Panels.

## Roadmap

### Phase 1: Multi-Channel-Output (nächste Session)
- `show_panel` Tool — separates Fenster mit Markdown-Rendering
- `ask_user` Tool — Multiple-Choice-Dialoge, per Klick und Sprache beantwortbar
- App-State-Tracking — Modell weiß, welche Panels/Dialoge offen sind
- Sprach-Shortcuts für Panel-/Dialog-Steuerung

### Phase 2: Voice Coding Basics
- Code-Dateien im Panel anzeigen mit Syntax-Highlighting
- Einfache Edits per Sprache ("ändere Zeile X", "füge ein nach Y")
- Git-Integration (Status, Diff, Commit per Sprache)
- Build/Test-Output im Panel

### Phase 3: Erweiterte Desktop-Steuerung
- Fenstermanagement per Sprache
- App-Start/Stop
- Erweiterte Dateisystem-Operationen
- Timer/Erinnerungen

### Phase 4: Cloud-Delegation
- Claude API / Codex-Integration für komplexe Code-Tasks
- Lokales Modell als Orchestrator, Cloud als Executor
- Ergebnis-Review und -Anwendung per Sprache

### Phase 5: Continuous Listening + Kontext-Persistenz
- VAD-basiertes freies Sprechen (kein PTT mehr nötig)
- Session-übergreifende Kontext-Persistenz
- Projekt-Awareness (weiß, in welchem Repo man arbeitet)
- Multi-Monitor-Awareness (Panel auf TV, Orb auf Laptop)
