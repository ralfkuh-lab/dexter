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

- [ ] **Wiederverbindung nach Neustart.** Bestehende dexter-* tmux-Sessions
  beim App-Start erkennen und Modus wiederherstellen.

## Visible Workspace (Phase 4 Ausbau)

- [ ] **Rechtes Seitenpanel.** Persistentes Fenster neben dem Orb mit
  aktuellem Modus, Agent-Status, und später Terminal-Output.

## Linux-Lücken

- [ ] **Active-Monitor-Detection auf Linux.** macOS hat das Python/Quartz-
  Snippet. Linux-Screenshot nimmt aktuell den ganzen virtuellen Desktop.
  Multi-Monitor-Auswahl wäre per `xrandr`-Geometrie machbar.

## Knowledge-Vault: mögliche Ausbauten (nicht dringend)

Basis steht: Markdown-Vault mit `search_notes`/`read_note`, Vault-Pfad in den
Settings (Knowledge-Tab). Denkbare Erweiterungen:

- [ ] **`[[wikilink]]`-Auflösung.** search_notes/read_note könnten verlinkte
  Notizen mit auflösen, damit das LLM Struktur folgen kann.

## Features / Ideen (nicht dringend)
