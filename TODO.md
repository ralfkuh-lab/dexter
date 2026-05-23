# TODO

Aktive Punkte. Erledigtes raus, nicht abhaken-und-stehenlassen.
Roadmap und Gesamtvision → `docs/VISION.md`.

## Phase 1: Multi-Channel-Output (nächste Priorität)

Implementierungsplan → `docs/PHASE1-PLAN.md`.

- [ ] **`show_panel`-Tool.** Separates Fenster mit Markdown-Rendering
  (react-markdown + remark-gfm). Für Dateilisten, Code, Tabellen, Diffs.
  Modell spricht kurze Zusammenfassung, Details im Panel.

- [ ] **Panel per Sprache schließen.** "Schließ das Panel" / "OK danke"
  wird vor dem LLM-Call abgefangen → Panel zu, kein LLM-Roundtrip.

- [ ] **App-State-Tracking.** UI-State (Panel offen, Dialog aktiv) als
  System-Message vor dem letzten User-Turn injizieren. Modell weiß, was
  auf dem Bildschirm los ist.

- [ ] **`ask_user`-Tool.** Multiple-Choice-Dialoge inline im Orb.
  Oneshot-Channel blockiert bis User per Klick oder Sprache antwortet
  (A/B/C/D, Zahlwörter, Label-Match). 60s Timeout.

## Linux-Lücken

- [ ] **Active-Monitor-Detection auf Linux.** macOS hat das Python/Quartz-
  Snippet. Linux-Screenshot nimmt aktuell den ganzen virtuellen Desktop.
  Multi-Monitor-Auswahl wäre per `xrandr`-Geometrie machbar.

## Aufräumen / kleine Refactorings

- [ ] **macOS-Pfade weiter pflegen, aber ungetestet.** Multi-Platform bleibt
  Ziel, aktive Entwicklung ist Linux. macOS-`cfg`-Branches bewusst behalten.

## Features / Ideen (nicht dringend)

- [ ] **Diagnose-Anzeige in Settings:** Config-Pfad und Endpunkt-Health live.

- [ ] **Mikrofon-Device wählbar machen.** Dropdown in Settings.

- [ ] **Freies-Sprechen-Modus (Mikro immer an).** VAD-Strategie, Satzende-
  Detection, Echo-Cancellation. Erst Konzept, dann bauen.
