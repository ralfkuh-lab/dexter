# TODO

Aktive Punkte. Erledigtes raus, nicht abhaken-und-stehenlassen.
Roadmap und Gesamtvision → `docs/VISION.md`.

## Tray-Icon mit Orb-Status (Custom Implementation)

- [ ] **Eigenes Tray-Icon mit libappindicator oder ksni.** Tauri's `tray-icon`
  Crate verdrahtet den `activate`-Signal nicht — Linksklick öffnet immer das
  Menü statt das Fenster. Eigene Implementation mit `connect_activate` für
  Linksklick (Fenster toggle) und Menü nur bei Rechtsklick. Dynamisches Icon
  das den Orb-State widerspiegelt (idle/listening/thinking/speaking). Ziel:
  Dexter läuft im Hintergrund, Orb-Icon zeigt Status, Fenster optional.

## Agent-Sessions (tmux-basiert)

- [ ] **Session-Cleanup beim App-Exit.** Offene tmux-Sessions beim Beenden
  von Dexter ordentlich aufräumen (kill_session für alle aktiven Modi).

- [ ] **Agent-Lifecycle erkennen.** Prüfen ob der Agent in der tmux-Session
  noch läuft oder beendet wurde. Bei Exit zurück in Chat-Modus wechseln.

- [ ] **Wiederverbindung nach Neustart.** Bestehende dexter-* tmux-Sessions
  beim App-Start erkennen und Modus wiederherstellen.

- [ ] **Terminal-Emulator konfigurierbar.** Aktuell hardcoded gnome-terminal,
  sollte über Config wählbar sein (kitty, alacritty, wezterm etc.).

- [ ] **macOS-Terminal-Integration.** Terminal.app oder iTerm2 statt
  gnome-terminal. tmux läuft auf macOS nativ.

## Visible Workspace (Phase 4 Ausbau)

- [ ] **Rechtes Seitenpanel.** Persistentes Fenster neben dem Orb mit
  aktuellem Modus, Agent-Status, und später Terminal-Output.

## Linux-Lücken

- [ ] **Active-Monitor-Detection auf Linux.** macOS hat das Python/Quartz-
  Snippet. Linux-Screenshot nimmt aktuell den ganzen virtuellen Desktop.
  Multi-Monitor-Auswahl wäre per `xrandr`-Geometrie machbar.

## Aufräumen / kleine Refactorings

- [ ] **macOS-Pfade weiter pflegen, aber ungetestet.** Multi-Platform bleibt
  Ziel, aktive Entwicklung ist Linux. macOS-`cfg`-Branches bewusst behalten.

- [ ] **Letzter Ollama-Rest: Vision-Pfad.** Chat-Client und RAG-Embeddings sind
  raus. Übrig ist nur `tools.rs::describe_screenshot` mit eigenem
  `provider == "ollama"`-Vision-Pfad (`/api/chat`) — nur aktiv bei Provider
  „ollama", kann bleiben oder später raus.

## Knowledge-Vault: mögliche Ausbauten (nicht dringend)

Basis steht: Markdown-Vault mit `search_notes`/`read_note`, Vault-Pfad in den
Settings (Knowledge-Tab). Denkbare Erweiterungen:

- [ ] **`[[wikilink]]`-Auflösung.** search_notes/read_note könnten verlinkte
  Notizen mit auflösen, damit das LLM Struktur folgen kann.
- [ ] **Synonym-/Mehrfachsuche.** Bei 0 Treffern das LLM zu alternativen
  Suchbegriffen anregen (Prompt-Hinweis), um die grep-Grenze abzufedern.

## Features / Ideen (nicht dringend)

- [ ] **Diagnose-Anzeige in Settings:** Config-Pfad und Endpunkt-Health live.

- [ ] **Mikrofon-Device wählbar machen.** Dropdown in Settings.
