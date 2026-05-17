"""OpenAI-kompatibler /v1/audio/speech-Endpunkt für Dexter mit Piper-TTS.

Drop-in-Ersatz für chatterbox-openai-server.py. Gleicher Port (8005), gleiches Schema.

Endpoints:
  POST /v1/audio/speech   Body: {"input": "...", "voice": "<name|path>"} -> WAV bytes
  GET  /health            -> {"ok": true, "default_voice": "..."}

Voice-Param:
  - leer / nicht gesetzt  -> Default-Stimme (DEFAULT_VOICE)
  - Dateiname (mit/ohne .onnx) -> aus VOICES_DIR geladen
  - absoluter Pfad        -> direkt geladen
"""

from __future__ import annotations

import io
import os
import wave
from pathlib import Path
from threading import Lock

from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse, Response
from pydantic import BaseModel
from piper import PiperVoice

VOICES_DIR = Path(os.environ.get("PIPER_VOICES_DIR", Path(__file__).resolve().parent.parent / "piper-voices"))
DEFAULT_VOICE = os.environ.get("PIPER_DEFAULT_VOICE", "de_DE-thorsten-medium")
HOST = os.environ.get("PIPER_HOST", "127.0.0.1")
PORT = int(os.environ.get("PIPER_PORT", "8005"))

app = FastAPI()
_voice_cache: dict[str, PiperVoice] = {}
_cache_lock = Lock()


def _resolve_voice(name: str | None) -> Path:
    candidates: list[str] = []
    if name:
        candidates.append(name)
    candidates.append(DEFAULT_VOICE)
    for cand in candidates:
        p = Path(cand)
        if p.is_absolute() and p.exists():
            return p
        local = VOICES_DIR / cand
        if local.suffix != ".onnx":
            local = local.with_suffix(".onnx")
        if local.exists():
            return local
    raise HTTPException(status_code=404, detail=f"no voice found (asked: {name!r}, default: {DEFAULT_VOICE!r})")


def _load_voice(model_path: Path) -> PiperVoice:
    key = str(model_path)
    with _cache_lock:
        v = _voice_cache.get(key)
        if v is None:
            v = PiperVoice.load(str(model_path))
            _voice_cache[key] = v
        return v


class SpeechRequest(BaseModel):
    input: str
    voice: str | None = None
    model: str | None = None  # ignored; kept for OpenAI-schema compatibility
    response_format: str | None = None
    speed: float | None = None


@app.get("/health")
def health() -> JSONResponse:
    default_path = _resolve_voice(None)
    return JSONResponse({
        "ok": True,
        "engine": "piper",
        "default_voice": default_path.name,
        "voices_loaded": list(_voice_cache.keys()),
    })


@app.post("/v1/audio/speech")
def speech(req: SpeechRequest) -> Response:
    text = (req.input or "").strip()
    if not text:
        raise HTTPException(status_code=400, detail="empty input")

    model_path = _resolve_voice(req.voice)
    voice = _load_voice(model_path)

    buf = io.BytesIO()
    with wave.open(buf, "wb") as wav_file:
        voice.synthesize_wav(text, wav_file)
    return Response(content=buf.getvalue(), media_type="audio/wav")


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host=HOST, port=PORT, log_level="info")
