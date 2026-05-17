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

- [ ] **Push-to-talk auf Wayland validieren.** Default ist `F9`
  (konfigurierbar in Settings) über `tauri-plugin-global-shortcut`. Wayland
  blockt globale Hotkeys teilweise und schluckt die Taste je nach Compositor
  nicht vor dem fokussierten Fenster. F9 ist Funktionstaste → kein Zeichen
  rutscht durch, aber „Hold" muss noch unter Wayland geprüft werden.

- [ ] **Diagnose-Anzeige in Settings:** aktueller Config-Pfad
  (`~/.config/voice-assistant/config.json`) und Erreichbarkeit der drei
  Endpunkte (STT/LLM/TTS) live in der UI.

- [ ] **Mikrofon-Device wählbar machen.** Aktuell nimmt `cpal` das System-
  Default-Input. Dropdown in Settings mit `cpal::Host::input_devices()`.

- [ ] **Texteingabe als Alternative zum Sprechen.** Toggle in der Orb-Leiste
  (Tastatur-Icon) und im Tray-Menü, der ein Eingabefeld unter dem Orb
  einblendet. Inhalt wird wie ein STT-Ergebnis in die Pipeline gefüttert,
  sodass Tools, Chat-History und TTS-Antwort wie gehabt laufen. Pragmatisch
  für laute Umgebungen und Debugging.

- [ ] **Freies-Sprechen-Modus (Mikro immer an).** Toggle (Mikro-Icon) nahe
  Orb + Menüpunkt. Offene Fragen: VAD-Strategie (Silero, WebRTC, oder
  Energy-Schwellwert in cpal), wer bestimmt Satzende (STT-Endpoint kann das
  nicht, müsste Client-seitig), Echo-Cancellation gegen den eigenen TTS-
  Output. Erst Konzept skizzieren, dann bauen.

- [ ] **Sprachausgabe an/aus.** Lautsprecher-Toggle nahe Orb + Menüpunkt.
  Wenn aus, läuft die Pipeline normal, aber der TTS-Step wird übersprungen
  und nur die Bubble angezeigt. Status-Feld in VoiceConfig, persistent.

- [ ] **`show_info`-Tool für formatierte Anzeige.** Neues Tool, das Markdown
  entgegennimmt und in einem eigenen Fenster (oder Overlay-Panel im Orb-
  Fenster) gerendert anzeigt. Use-Cases: Tabellen, lange Listen, Code-
  Snippets, Ergebnisse aus Web-Fetch/Shell. Frontend braucht einen Renderer
  (z.B. react-markdown + Prism). Sprachausgabe parallel gibt eine knappe
  Zusammenfassung, das Fenster zeigt das Detail.
