# Dexter Architektur-Refactoring

## Ziel

Dexter soll schrittweise zu einer sauber modularisierten, voice-first Desktop-
Steuerung werden. Die Codebasis muss dabei fuer KI-Agenten gut lesbar bleiben:
klare Modulgrenzen, expliziter State, kleine Integrationspunkte und testbare
Routing-Entscheidungen.

Wichtig fuer den Coding-Workflow: Dexter ist nicht das Coding-Modell. Dexter
bedient CLI-Agenten wie Codex CLI, Claude Code CLI, opencode oder agy. Fachliche
Coding-Arbeit, Reviews, Refactorings, Undo-Strategien und Commits bleiben bei
diesen Agenten. Dexter uebernimmt Voice-Eingabe, Routing, Fenstersteuerung,
Rueckfragen und einfache manuelle Hilfsaktionen wie Build/Test/App-Start.

## Reihenfolge

### Phase 0: Automation API als Sicherheitsnetz

Vor groesseren Umbauten bekommt Dexter eine kleine lokale Automation API. Damit
koennen Agenten die App starten, Eingaben einspeisen, Dialoge beantworten,
Panels pruefen und auf stabile Zustaende warten.

Ergebnis:
- lokaler HTTP-Server nur auf `127.0.0.1`
- stabile JSON-Endpunkte fuer State, Texteingabe, Dialogantworten und Waits
- Backend-State fuer aktuellen Pipeline-Zustand
- erste Smoke-Tests als Grundlage fuer spaetere Refactorings

Details stehen in `docs/AUTOMATION-API-PLAN.md`.

### Phase 1: Stabilitaetsfixes

Diese Punkte haben geringe fachliche Breite, aber hohe Zuverlaessigkeit:

- Sandbox-Timeout beendet den Child-Prozess wirklich.
- Text-Truncation und Chunking werden Unicode-sicher.
- Mouse-PTT bricht laufende Pipelines genauso ab wie Hotkey-PTT.
- Bestehende Tests und Automation-Smokes sichern das Verhalten.

### Phase 2: Application Modes und Input Router

Der aktive Modus entscheidet, wohin normale Sprache geht:

- `CHAT`: Dexter verarbeitet die Eingabe selbst.
- `CODEX_SESSION`: Eingabe geht direkt an die aktive Codex-CLI-Session.
- `CLAUDE_SESSION`: Eingabe geht direkt an Claude Code CLI.
- spaeter: `OPENCODE_SESSION`, `AGY_SESSION`, Browser, Shell, Media.

`KOMMANDO ...` ist ein deterministischer Control Channel und wird immer von
Dexter selbst ausgewertet. Dieser Parser gehoert nicht in die LLM-Pipeline.

Neue Bausteine:
- `AppMode`
- `InputRouter`
- `CommandParser`
- explizite Mode-Events fuer UI und Automation

### Phase 3: Pipeline modularisieren

`pipeline.rs` ist aktuell der zentrale Integrationspunkt fuer STT, LLM, TTS,
Tool-Ausfuehrung, UI-Kommandos, Panel und Dialoge. Das bleibt kurzfristig
funktional, soll aber in kleinere Module aufgeteilt werden:

- `conversation`: Message-History, volatile Tool-Redaction, UI-Kontext
- `tool_executor`: Tool-Dispatch und Tool-Result-Aufbereitung
- `panel_manager`: Panel-State und Tauri-Fenster
- `dialog_manager`: `ask_user`, Voice/Klick-Resolution, Timeout
- `input_router`: Mode-Routing vor der LLM-Pipeline
- `pipeline`: nur noch Orchestrierung der Audio/Text-Verarbeitung

### Phase 4: Sichtbarer Workspace

Dexter bekommt ein dauerhaft sichtbares rechtes Seitenfenster:

- Appstate/Betriebsmodus farbig ganz oben
- Chat/Status/Agenten-Output darunter
- Arbeitsfenster oder CLI links
- Panels und Rueckfragen als Vordergrund-Popups

Das Layout muss aus der App heraus wiederherstellbar sein, damit der Anwender
jederzeit sieht, wohin die naechste Spracheingabe geroutet wird.

### Phase 5: CLI-Agent Sessions

Erst nach sauberem State und Routing kommt die Agenten-Integration:

- laufende CLI-Agenten-Sessions erkennen oder starten
- PTY/Terminal-Bridge fuer Codex, Claude, opencode, agy
- Prompts und Auswahlantworten direkt an aktive Session senden
- Agenten-Rueckfragen im Dexter-Dialog anzeigen
- Output/Diffs/Logs im Panel zeigen

Dexter soll hier nur bedienen und vermitteln. Die eigentliche Coding-Intelligenz
bleibt in den spezialisierten CLI-Agenten.

## Akzeptanzkriterien

- Der aktive Modus ist im Backend-State, in der UI und in der Automation API
  sichtbar.
- Normale Eingaben werden genau einmal geroutet: entweder an Dexter oder an eine
  aktive Agenten-Session.
- `KOMMANDO ...` wird ohne LLM-Freilauf verarbeitet.
- Groessere Refactorings werden durch Automation-Smokes abgesichert.
- Neue Module haben klare Verantwortung und koennen von KI-Agenten ohne
  Kontextsuche verstanden werden.
