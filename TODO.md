# TODO

Aktive Punkte. Erledigtes raus, nicht abhaken-und-stehenlassen.

## Linux-Lücken (aktuell nur Stub oder kaputt)

- [ ] **Active-Monitor-Detection auf Linux.** macOS hat das Python/Quartz-
  Snippet. Linux-Screenshot nimmt aktuell den ganzen virtuellen Desktop
  (Default des aufgerufenen Tools). Multi-Monitor-Auswahl wäre per
  `grim -o <output>` (Wayland) bzw. `xrandr`-Geometrie + `import -window`
  oder Crop nach dem Capture machbar.

## Aufräumen / kleine Refactorings

- [ ] **macOS-Pfade weiter pflegen, aber ungetestet.** Multi-Platform bleibt
  Ziel, aktive Entwicklung ist Linux. macOS-`cfg`-Branches in `tools.rs`,
  `sandbox.rs` und `lib.rs` bewusst behalten. Bei nächster Mac-Session erst
  durchsmoken bevor irgendwas committed wird, das daran rührt.

## Features / Ideen (nicht dringend)

- [ ] **Diagnose-Anzeige in Settings:** aktueller Config-Pfad
  (`~/.config/voice-assistant/config.json`) und Erreichbarkeit der drei
  Endpunkte (STT/LLM/TTS) live in der UI.

- [ ] **Mikrofon-Device wählbar machen.** Aktuell nimmt `cpal` das System-
  Default-Input. Dropdown in Settings mit `cpal::Host::input_devices()`.

- [ ] **Freies-Sprechen-Modus (Mikro immer an).** Toggle (Mikro-Icon) nahe
  Orb + Menüpunkt. Offene Fragen: VAD-Strategie (Silero, WebRTC, oder
  Energy-Schwellwert in cpal), wer bestimmt Satzende (STT-Endpoint kann das
  nicht, müsste Client-seitig), Echo-Cancellation gegen den eigenen TTS-
  Output. Erst Konzept skizzieren, dann bauen.

- [ ] **`ask_user`-Tool für Rückfragen.** Wenn Dexter unsicher ist, was
  gemeint ist (z.B. mehrdeutiger STT-Output, mehrere passende Dateien),
  soll er per Tool eine Auswahl-/Rückfrage-UI im Frontend triggern können
  statt verbal zurückzufragen. Braucht: Tauri-Command, Frontend-Dialog
  (Modal oder Inline im Orb), Tool-Definition + Prompt-Instruktion.

- [ ] **`show_info`-Tool für formatierte Anzeige.** Neues Tool, das Markdown
  entgegennimmt und in einem eigenen Fenster (oder Overlay-Panel im Orb-
  Fenster) gerendert anzeigt. Use-Cases: Tabellen, lange Listen, Code-
  Snippets, Ergebnisse aus Web-Fetch/Shell. Frontend braucht einen Renderer
  (z.B. react-markdown + Prism). Sprachausgabe parallel gibt eine knappe
  Zusammenfassung, das Fenster zeigt das Detail.
