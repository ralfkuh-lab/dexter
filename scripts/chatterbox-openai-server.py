#!/usr/bin/env python3
"""Small OpenAI-compatible speech endpoint for Dexter using Chatterbox."""

from __future__ import annotations

import argparse
import io
import os
import threading
import wave
from pathlib import Path
from typing import Literal

import torch
from fastapi import FastAPI, HTTPException
from fastapi.responses import Response
from pydantic import BaseModel


class SpeechRequest(BaseModel):
    input: str
    voice: str | None = None
    model: str | None = "chatterbox"


class ChatterboxService:
    def __init__(self, engine: Literal["standard", "turbo"], device: str, voice_dir: Path):
        self.engine = engine
        self.device = device
        self.voice_dir = voice_dir
        self.model = None
        self.lock = threading.Lock()

    def load(self) -> None:
        if self.model is not None:
            return
        if self.engine == "turbo":
            from chatterbox.tts_turbo import ChatterboxTurboTTS

            self.model = ChatterboxTurboTTS.from_pretrained(self.device)
        else:
            from chatterbox.tts import ChatterboxTTS

            self.model = ChatterboxTTS.from_pretrained(device=self.device)

    def synthesize(self, text: str, voice: str | None) -> bytes:
        if not text.strip():
            raise ValueError("input is empty")
        self.load()
        assert self.model is not None

        audio_prompt_path = self._resolve_voice(voice)
        kwargs = {}
        if audio_prompt_path is not None:
            kwargs["audio_prompt_path"] = str(audio_prompt_path)
        elif self.engine == "turbo":
            raise ValueError("Chatterbox Turbo requires a reference voice WAV")

        with self.lock:
            wav_tensor = self.model.generate(text, **kwargs)
        return wav_tensor_to_wav_bytes(wav_tensor, int(self.model.sr))

    def _resolve_voice(self, voice: str | None) -> Path | None:
        if not voice:
            return None
        path = Path(voice).expanduser()
        if path.is_file():
            return path
        candidate = self.voice_dir / voice
        if candidate.is_file():
            return candidate
        return None


def auto_device() -> str:
    if torch.cuda.is_available():
        return "cuda"
    if hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def wav_tensor_to_wav_bytes(wav_tensor: torch.Tensor, sample_rate: int) -> bytes:
    wav = wav_tensor.detach().cpu().float()
    if wav.ndim == 1:
        wav = wav.unsqueeze(0)
    if wav.shape[0] > wav.shape[1]:
        wav = wav.T
    wav = wav.clamp(-1.0, 1.0)
    pcm = (wav.squeeze(0).numpy() * 32767.0).astype("<i2")

    out = io.BytesIO()
    with wave.open(out, "wb") as wav_file:
        wav_file.setnchannels(1)
        wav_file.setsampwidth(2)
        wav_file.setframerate(sample_rate)
        wav_file.writeframes(pcm.tobytes())
    return out.getvalue()


def create_app(service: ChatterboxService) -> FastAPI:
    app = FastAPI(title="Dexter Chatterbox Speech Server")

    @app.get("/health")
    def health() -> dict[str, str | bool]:
        return {
            "ok": True,
            "engine": service.engine,
            "device": service.device,
            "model_loaded": service.model is not None,
        }

    @app.post("/v1/audio/speech")
    def speech(request: SpeechRequest) -> Response:
        try:
            audio = service.synthesize(request.input, request.voice)
        except Exception as exc:
            raise HTTPException(status_code=500, detail=str(exc)) from exc
        return Response(content=audio, media_type="audio/wav")

    return app


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default=os.getenv("DEXTER_CHATTERBOX_HOST", "127.0.0.1"))
    parser.add_argument("--port", type=int, default=int(os.getenv("DEXTER_CHATTERBOX_PORT", "8005")))
    parser.add_argument(
        "--engine",
        choices=["standard", "turbo"],
        default=os.getenv("DEXTER_CHATTERBOX_ENGINE", "standard"),
    )
    parser.add_argument("--device", default=os.getenv("DEXTER_CHATTERBOX_DEVICE", "auto"))
    parser.add_argument(
        "--voice-dir",
        type=Path,
        default=Path(os.getenv("DEXTER_CHATTERBOX_VOICE_DIR", "voices")),
    )
    return parser.parse_args()


args = parse_args()
device = auto_device() if args.device == "auto" else args.device
service = ChatterboxService(args.engine, device, args.voice_dir.expanduser().resolve())
app = create_app(service)


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host=args.host, port=args.port)
