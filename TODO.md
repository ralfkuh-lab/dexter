# TODO

Aktive Punkte. Erledigtes raus, nicht abhaken-und-stehenlassen.

## Bugs / kleine Korrekturen

- [ ] **Tool-Beschreibungen lecken macOS-Wording in den LLM-Kontext.**
  `voice.rs` build_tools sagt dem Modell „on the user's Mac" (Z.363, Z.397)
  und „runs in zsh" (Z.403). Auf Linux ist das falsch und kann den Assistant
  verwirren. → Plattform-neutral umformulieren oder per `cfg!` einsetzen.

- [ ] **User-Agent im `web_fetch` lügt.** `tools.rs:254` sendet eine Mac-Safari-
  UA. Auf einen neutralen oder Linux-Firefox-String ändern.

- [ ] **`list_running_apps` Tool-Description sagt „on your Mac".** Same as oben.

## Linux-Lücken (aktuell nur Stub oder kaputt)

- [ ] **Screenshot-Tool tut auf Linux nichts.** `tools.rs:64` gibt nur
  „not implemented for this platform" zurück. Backend bauen:
  Wayland (XDG-Desktop-Portal `org.freedesktop.portal.Screenshot`) primär,
  `grim`/`slurp` als wlroots-Fallback, X11 via `xwd` oder native crate.
  Output-Contract identisch zur macOS-Variante: base64 JPEG, max 1280px,
  Qualität ~70.

- [ ] **Active-Monitor-Detection auf Linux.** macOS hat das Python/Quartz-
  Snippet. Auf Linux Multi-Monitor erstmal Monitor 0 nehmen, später besser.

## Aufräumen / kleine Refactorings

- [ ] **macOS-Pfade weiter pflegen, aber ungetestet.** Multi-Platform bleibt
  Ziel, aktive Entwicklung ist Linux. macOS-`cfg`-Branches in `tools.rs`,
  `sandbox.rs` und `lib.rs` bewusst behalten. Bei nächster Mac-Session erst
  durchsmoken bevor irgendwas committed wird, das daran rührt.

## Features / Ideen (nicht dringend)

- [ ] **Push-to-talk auf Wayland validieren.** Aktuell `Shift+Z` über
  `tauri-plugin-global-shortcut`. Wayland blockt globale Hotkeys teilweise.
  Falls unzuverlässig → konfigurierbare Toggle-to-talk-Variante als Fallback.

- [ ] **Diagnose-Anzeige in Settings:** aktueller Config-Pfad
  (`~/.config/voice-assistant/config.json`) und Erreichbarkeit der drei
  Endpunkte (STT/LLM/TTS) live in der UI.

- [ ] **Mikrofon-Device wählbar machen.** Aktuell nimmt `cpal` das System-
  Default-Input. Dropdown in Settings mit `cpal::Host::input_devices()`.
