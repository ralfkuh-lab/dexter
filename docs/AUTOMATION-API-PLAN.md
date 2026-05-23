# Dexter Automation API

## Zweck

Die Automation API ist ein lokales Test- und Steuerungsinterface fuer Dexter.
Sie soll Agenten erlauben, die App waehrend Refactorings selbst zu bedienen:
Texte einspeisen, PTT-Flows ausloesen, Dialoge beantworten, Panels schliessen,
State abfragen und auf stabile Zustaende warten.

Die API ist kein Remote-Control-Feature fuer andere Rechner. Sie bindet nur an
`127.0.0.1`.

## Referenzmuster

Das Muster orientiert sich an der Folio Automation API:

- lokaler Axum-Server neben Tauri
- Loopback-only Bind
- JSON-Endpunkte
- stabile Event-Namen als Integrationsvertrag
- optionale Frontend-Acks fuer UI-mutierende Aktionen
- kleiner Python-Client fuer E2E-Smokes

Dexter uebernimmt das Muster reduziert. Es wird nicht die komplette Folio-API
kopiert.

## Port und Aktivierung

Default-Port:

```text
127.0.0.1:9877
```

Spaeter kann der Port ueber `DEXTER_AUTOMATION_PORT` oder Config steuerbar
werden. In der ersten Version reicht ein fester Port mit sauberem Fehlerlog,
falls er belegt ist.

## Erste Endpunkte

### `GET /state`

Liefert einen kompakten, maschinenlesbaren Zustand:

- aktueller Pipeline-State
- `is_recording`
- Anzahl Chat-Messages
- letzte Chat-Messages
- Panel offen: Titel und Content-Laenge
- Dialog offen: Frage und Optionen
- TTS aktiviert
- aktuelles Modell
- spaeter: `app_mode`, aktive Agenten-Session, Fensterlayout

### `GET /events`

Liefert die letzten Backend-/Automation-Events. Das ist bewusst eine kleine
Debug-Spur, kein vollwertiges Logging-System.

### `GET /console/errors`

Liefert vom Frontend gemeldete `console.error`, `window.error` und
`unhandledrejection` Eintraege. Dieser Endpunkt hilft Agenten, UI-Fehler nach
Refactorings zu erkennen.

### `POST /text`

Body:

```json
{ "text": "Zeig mir die aktuelle Uhrzeit" }
```

Speist Text in denselben Pfad ein wie die manuelle Texteingabe. Das ist der
wichtigste Automation-Einstieg, weil er STT ueberspringt, aber LLM/Tools/Dialoge
real durchlaeuft.

### `POST /ptt/press` und `POST /ptt/release`

Loest dieselben Backend-Pfade aus wie der globale PTT-Hotkey. Damit koennen
Recording- und Interrupt-Verhalten getestet werden.

### `POST /ptt/cancel`

Stoppt eine laufende Aufnahme, leert die aufgenommenen Samples und setzt den
State auf `idle`, ohne STT/Whisper anzustossen. Dieser Pfad ist fuer
Automation-Smokes gedacht, die keinen laufenden Voice-Stack voraussetzen.

### `POST /dialog/answer`

Body:

```json
{ "selected": "A" }
```

Beantwortet den aktuell offenen `ask_user`-Dialog. Akzeptiert dieselben
Auswahlformen wie Sprache: A-D, 1-4, deutsche Zahlwoerter oder Label-Match.

### `POST /panel/close`

Schliesst das Panel-Fenster und leert den Panel-State.

### `POST /wait`

Body:

```json
{ "condition": "idle", "timeout_ms": 10000 }
```

Unterstuetzte Bedingungen in Version 1:

- `idle`
- `recording`
- `dialog.shown`
- `panel.open`
- `messages.changed`

### `POST /quit`

Beendet die App. Dieser Endpunkt ist fuer E2E-Smokes nuetzlich.

## Frontend-Acks

Nicht jeder Endpunkt braucht ein Ack. Backend-only Aktionen wie `/text` oder
`/state` koennen direkt antworten.

Fuer spaetere UI-mutierende Events soll das Folio-Muster uebernommen werden:

- Backend erzeugt `request_id`
- Backend emittiert Tauri-Event mit `requestId`
- Frontend fuehrt DOM/UI-Aktion aus
- Frontend ruft `automation_ack(requestId)`
- HTTP-Handler wartet mit Timeout

Erste Kandidaten:

- Klicks auf UI-Elemente
- Text in sichtbare Eingabefelder setzen
- Fensterlayout pruefen oder herstellen
- Panel-Rendering bestaetigen

## Teststruktur

Erste Dateien:

```text
src-tauri/src/automation.rs
src/automation/console.ts
tests/e2e/lib/api.py
tests/e2e/smoke_automation.py
```

Der erste Smoke-Test soll ohne Voice-Stack laufen:

1. `/state` ist erreichbar.
2. `/text` mit leerem Text wird abgelehnt.
3. `/panel/close` ist idempotent.
4. `/wait` auf `idle` funktioniert.

Weitere Smokes koennen spaeter gegen einen laufenden lokalen AI-Stack testen.

## Grenzen

- Keine Netzwerkbindung ausser Loopback.
- Keine komplexen Browser-/DOM-Aktionen in Version 1.
- Keine eigene Coding-Logik.
- Keine direkte Umgehung der bestehenden Tauri-Commands, wenn es bereits einen
  sauberen Backend-Pfad gibt.
