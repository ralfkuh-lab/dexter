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

- Welcher Betriebsmodus aktiv ist (`CHAT`, `CODEX_SESSION`,
  `CLAUDE_SESSION`, `OPENCODE_SESSION`, ...)
- Welche Panels/Dialoge sind offen
- Welche CLI-Agenten-Sessions laufen und welche davon aktiv ist
- Welche Dateien wurden zuletzt bearbeitet
- Welcher Build-Status aktiv ist
- Was der User zuletzt per Screenshot gesehen hat

Der UI-State wird als kontextuelle System-Message vor dem letzten User-Turn
injiziert, nicht im System-Prompt (um den Prompt-Cache nicht zu brechen).

### 3. Application Modes & Voice Routing

Dexter arbeitet modal. Der aktive Application State entscheidet, wohin normale
Spracheingaben geroutet werden:

- **`CHAT`** — Default-Modus. Dexter verarbeitet Eingaben selbst, antwortet
  kurz per Sprache und nutzt lokale Tools wie Panels, Screenshot, Clipboard,
  Web-Fetch oder einfache Desktop-Aktionen.
- **`CODEX_SESSION`** — Alle normalen Eingaben gehen direkt an die aktive
  Codex-CLI-Session. Dexter ist nur Voice-Eingabe, Output-Anzeige und
  Rückfragen-Vermittler.
- **`CLAUDE_SESSION`**, **`OPENCODE_SESSION`**, **`AGY_SESSION`** — analog für
  weitere CLI-Agenten.
- **Weitere Modi** können später Browser-, Shell-, Media- oder Desktop-Control
  abbilden, aber immer mit klarer Routing-Regel.

Normale Eingaben werden im aktiven Modus nicht jedes Mal neu interpretiert. Wenn
`CODEX_SESSION` aktiv ist, bedeutet ein Satz wie "ändere die Funktion so, dass
..." automatisch: an Codex senden. Der User muss nicht ständig "Sag Codex ..."
voranstellen.

**Control Channel:** Sätze mit `KOMMANDO ...` werden immer von Dexter selbst
ausgewertet und niemals an die aktive Agenten-Session weitergeleitet. Dieser
Kanal ist deterministisch, schnell und möglichst ohne LLM-Interpretation:

- "KOMMANDO chat" → zurück in den Default-Modus
- "KOMMANDO coding session codex" → Codex-Session aktivieren oder starten
- "KOMMANDO coding session claude" → Claude-Code-Session aktivieren oder starten
- "KOMMANDO status" → aktuellen Modus und aktive Session anzeigen
- "KOMMANDO lies letzte ausgabe" → aktuellen CLI-Output zusammenfassen/vorlesen
- "KOMMANDO stopp" → laufende Ausgabe oder aktive Operation abbrechen

STT-Varianten wie "commando", "komando" oder "Dexter Kommando" sollten tolerant
erkannt werden. Unklare Kommandos führen zu einer kurzen Rückfrage, nicht zu
einem LLM-Freilauf.

### 4. Visible Workspace Layout

Dexter soll nicht nur ein kurz eingeblendeter Orb sein, sondern standardmäßig
einen sichtbaren Arbeitsbereich anbieten:

- **Dexter-Fenster rechts:** ein dauerhaft sichtbares Seitenfenster am rechten
  Bildschirmrand. Ganz oben steht der aktuelle Appstate/Betriebsmodus, farblich
  hinterlegt und sofort erkennbar, z.B. `CHAT`, `CODEX`, `CLAUDE`, `BROWSER`.
- **Arbeitsfenster links:** die gerade bediente Anwendung oder CLI-Session
  nimmt links den restlichen Platz ein. Das kann ein Terminal mit Codex,
  Claude Code, opencode oder agy sein, später auch Browser oder andere Apps.
- **Default-Anordnung:** Dexter kann diese Fensteranordnung selbst herstellen:
  Seitenfenster rechts andocken, aktives Arbeitsfenster links positionieren,
  Größen sinnvoll setzen und bei Bedarf wiederherstellen.
- **Popups im Vordergrund:** Info-Panels, Rückfragen, Bestätigungsdialoge und
  Feedback-Fenster dürfen als Vordergrund-Popups erscheinen, ohne die Grundlogik
  des rechten Dexter-Seitenfensters zu ersetzen.

Der User soll jederzeit sehen können, in welchem Modus Dexter ist und wohin die
nächste Spracheingabe geht. Das ist besonders wichtig, weil im Agentenmodus
normale Sprache direkt an eine CLI-Session gesendet wird.

### 5. Voice-First Interaction Patterns

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

### 6. Voice CLI-Agent Workflow

Das ambitionierteste Feature: komplette CLI-Agent-Sessions per Sprache bedienen.

Wichtig: Dexter soll **nicht** das lokale Gemma-Modell zum Programmieren
verwenden. Gemma bleibt der schnelle lokale Sprach-Orchestrator: Es versteht den
User, entscheidet über lokale Tools, steuert Panels/Dialoge und führt einfache
Desktop-Aktionen aus. Für echte Coding-Arbeit soll Dexter die dafür gebauten
Agenten-CLIs bedienen — insbesondere **Claude Code CLI**, **Codex CLI**,
**opencode**, **agy** und ähnliche Terminal-Agenten.

Der Workflow ist also: Stimme → Dexter → bestehende CLI-Session → Agent
arbeitet. Dexter ist dabei nur die Voice-Bedienoberfläche: Es sendet Prompts,
Kommandos und Auswahlantworten an die CLI, zeigt deren Output im Panel, liest
Rückfragen vor und reicht die Antwort des Users zurück. Dexter soll Code,
Reviews, Refactorings, Undo-Strategien, Commits und andere fachliche
Coding-Entscheidungen nicht selbst übernehmen.

**CLI-Agenten bedienen:**
- Im `CODEX_SESSION`-Modus: "Ändere die Funktion default_prompt ..." → Dexter
  sendet den Text direkt an die laufende Codex-CLI-Session
- Im `CLAUDE_SESSION`-Modus: "Schau dir den Diff an und committe sauber" →
  Dexter sendet den Text direkt an Claude Code CLI
- "Antworte Claude mit Option B" → Dexter reicht die Auswahl an die
  Claude-Code-CLI weiter
- "Lies mir die Rückfrage vor" → Dexter fasst die aktuelle CLI-Rückfrage kurz
  per Sprache zusammen
- "Zeig mir den aktuellen Agent-Output" → Panel zeigt den Terminal-Output

**Manuelle Hilfsaktionen:**
- "Öffne die Datei config.rs" → Panel zeigt den Inhalt, ohne daraus selbst eine
  Änderung abzuleiten
- "Ändere die Funktion default_prompt, ersetze den Rückgabewert durch ..." →
  Dexter leitet den Auftrag an den gewählten CLI-Agenten weiter
- "Mach das rückgängig" → Dexter gibt den Wunsch an den CLI-Agenten weiter;
  die Undo-Strategie entscheidet der Agent

**Code reviewen:**
- "Zeig mir den Diff seit dem letzten Commit" → Panel mit farbigem Diff
- "Was hat Codex gerade geändert?" → Dexter zeigt den Agent-Output oder Diff,
  die Bewertung übernimmt Codex/Claude/opencode/agy
- "Sag Claude: sieht gut aus, committe das mit der Message 'Fix XY'" → Dexter
  sendet diese Anweisung an Claude Code CLI; Dexter committet nicht selbst

**Build und Test:**
- "Kompilier das mal" → Dexter stößt den Build an, wie der User es manuell im
  Terminal tun würde, und zeigt den Output im Panel
- "Starte die App" → Dexter startet die App oder Dev-Server
- "Lauf die Tests" → Dexter kann Tests starten und Output anzeigen; Analyse und
  Fix-Vorschläge gehen an den CLI-Agenten

**Navigation:**
- "Welche Dateien gibt es im src-Verzeichnis?" → Panel mit Baumstruktur
- "Zeig mir die Funktion handle_ptt_release" → Panel springt zur Stelle
- "Wo wird AppState verwendet?" → grep-Ergebnis im Panel

**Grenze der Verantwortung:**
- Dexter macht im Coding-Kontext nur das, was der User sonst manuell im
  Terminal oder in der CLI-Session tun würde: Text senden, Auswahl treffen,
  Build/Test/App-Start anstoßen, Output anzeigen, Fenster/Panel bedienen.
- Alle fachlichen Coding-Aufgaben bleiben bei den fähigen Agentenmodellen in
  ihren CLI-Versionen: Codex, Claude Code, opencode, agy und ähnliche Tools.
- Auch Commits, Reviews, Refactorings und komplexe Git-Entscheidungen sollen
  von diesen Agenten übernommen werden, nicht von Dexter.

### 7. Desktop-Steuerung

Über das Coding hinaus:

- **App-Steuerung** — "Öffne Firefox", "Schließ das Terminal", "Wechsel zu VS Code"
- **System-Info** — "Wie viel Speicher ist noch frei?", "Welche Prozesse fressen CPU?"
- **Dateisystem** — "Verschieb die Datei nach Downloads", "Lösch die alten Logs"
- **Web** — "Such mal nach Rust async patterns", "Was steht auf der Seite?"
- **Benachrichtigungen** — "Erinner mich in 30 Minuten an den Build"

### 8. Sicherheit

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
  ~60-100 tok/s. Für Voice-Orchestrierung, Tool-Auswahl und einfache
  Desktop-Aufgaben ausreichend, aber nicht als Coding-Modell gedacht.
- **Coding-Agenten:** Claude Code CLI, Codex CLI, opencode, agy und ähnliche
  Terminal-Agenten übernehmen Code-Generierung, große Kontexte, Refactorings,
  Reviews und Commits. Dexter bedient nur die CLI-Sessions per Sprache.
- **STT:** Whisper large-v3-turbo, lokal auf CUDA, ~1s Latenz.
- **TTS:** Piper (CPU, Deutsch), ~50ms pro Satz.
- **Display:** Laptop-Bildschirm oder TV als zweiter Monitor. Dexter sollte
  standardmäßig als rechtes Seitenfenster sichtbar bleiben; die aktive
  Arbeits-App oder CLI-Session nimmt links den restlichen Platz ein.

## Roadmap

### Phase 1: Multi-Channel-Output
- `show_panel` Tool — separates Fenster mit Markdown-Rendering
- `ask_user` Tool — Multiple-Choice-Dialoge, per Klick und Sprache beantwortbar
- App-State-Tracking — Dexter weiß, welche Panels/Dialoge offen sind
- Sprach-Shortcuts für Panel-/Dialog-Steuerung

### Phase 2: Application Modes & Visible Workspace
- `CHAT` als Default-Modus
- `KOMMANDO ...` Control Channel mit deterministischem Parser
- Moduswechsel per Sprache: `KOMMANDO chat`,
  `KOMMANDO coding session codex`, `KOMMANDO coding session claude`
- Rechtes Dexter-Seitenfenster mit farbigem Appstate oben
- Default-Layout: Dexter rechts, aktive Arbeits-App/CLI links

### Phase 3: Voice CLI Control Basics
- Laufende CLI-Agenten-Sessions erkennen und auswählen
- Prompts und Antworten per Sprache an Codex/Claude/opencode/agy senden
- Agent-Rückfragen als Vordergrund-Popup anzeigen und per Sprache/Klick
  beantworten
- Build/Test/App-Start als manuelle Hilfsaktionen anstoßen
- CLI-Output, Diffs und Logs im Panel anzeigen

### Phase 4: Agenten-CLI-Orchestrierung
- Robuste Claude Code CLI / Codex CLI / opencode / agy Integration
- Mehrere parallele CLI-Sessions benennen, fokussieren und bedienen
- Rückfragen, Bestätigungen und längere Outputs zwischen User und Agent
  vermitteln

### Phase 5: Erweiterte Desktop-Steuerung
- Fenstermanagement per Sprache
- App-Start/Stop
- Erweiterte Dateisystem-Operationen
- Timer/Erinnerungen

### Phase 6: Continuous Listening + Kontext-Persistenz
- VAD-basiertes freies Sprechen (kein PTT mehr nötig)
- Session-übergreifende Kontext-Persistenz
- Projekt-Awareness (weiß, in welchem Repo man arbeitet)
- Multi-Monitor-Awareness (Dexter rechts, Arbeitsfenster links oder auf TV)
