"""The HTTP API, built by create_app() from the services it needs (see main.py for the real ones)."""
import logging
import os
import socket
import tempfile
from collections.abc import Callable
from contextlib import asynccontextmanager
from dataclasses import dataclass, field
from typing import Protocol

from fastapi import FastAPI, File, HTTPException, Request, UploadFile
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import JSONResponse
from pydantic import BaseModel

from saambha import history
from saambha.server.auth import bearer_token, is_local

log = logging.getLogger(__name__)


class Assistant(Protocol):
    """An agent harness. ask() calls on_event("text" | "tool_use", detail) as the agent works."""
    def ask(self, text: str, on_event: Callable[[str, str], None] | None = None) -> str: ...
    def reset(self): ...
    def has_session(self) -> bool: ...


class Devices(Protocol):
    def is_valid(self, token: str | None) -> bool: ...


class Pairing(Protocol):
    async def request(self, ip: str, device_name: str) -> str | None: ...


class Speaker(Protocol):
    def speak(self, text: str): ...
    def stop(self): ...


class Status(Protocol):
    """What Saambha is doing, shown by the desktop app, which also answers whether to allow a new device."""
    def set_listening(self, listening: bool): ...
    def set_task(self, task: str | None): ...
    def snapshot(self) -> dict: ...
    async def next(self, after: int) -> dict: ...
    def answer(self, allowed: bool) -> bool: ...


class History(Protocol):
    def add(self, said: str, reply: str): ...
    def start_conversation(self): ...
    def recent(self, limit: int) -> list: ...


class Log(Protocol):
    def read(self, offset: int | None) -> tuple[str, int]: ...


class Transcriber(Protocol):
    def transcribe(self, audio_path: str) -> str: ...


@dataclass
class Services:
    assistant: Assistant
    devices: Devices
    pairing: Pairing
    speaker: Speaker
    status: Status
    history: History
    log: Log
    transcriber: Transcriber
    hostname: str = field(default_factory=socket.gethostname)
    on_startup: Callable[[], None] = lambda: None
    on_shutdown: Callable[[], None] = lambda: None


class PairRequest(BaseModel):
    name: str = "Unknown device"


class PairingAnswer(BaseModel):
    allow: bool


class CommandRequest(BaseModel):
    text: str
    speak: str = "desktop"  # "desktop": speak the reply here; "none": the app speaks it


# Reachable without a token: the apps' LAN scan and the pairing request.
PUBLIC_PATHS = {"/discover", "/pair"}
# Only for the desktop app on this machine: not even paired devices (they could allow others).
DESKTOP_PATHS = "/desktop/"


def create_app(services: Services) -> FastAPI:
    s = services

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        s.on_startup()
        yield
        s.on_shutdown()

    app = FastAPI(lifespan=lifespan)

    def run_command(text: str, speak_on_desktop: bool = True) -> dict:
        log.info(f'INPUT: "{text}"')

        def on_event(event_type, content):
            if event_type == "text":
                log.info(f"STREAM text: {content!r}")
            elif event_type == "tool_use":
                log.info(f"STREAM tool: {content}")
                s.status.set_task(f"Running {content}")

        s.status.set_task("Thinking")
        try:
            response_text = s.assistant.ask(text, on_event=on_event)
            log.info(f"RESPONSE: {response_text!r}")
            s.history.add(text, response_text)  # before the change that tells the panel to show it
        finally:
            s.status.set_task(None)
        if response_text and speak_on_desktop:
            s.speaker.speak(response_text)
        return {"input": text, "text": response_text}

    @app.middleware("http")
    async def require_paired_device(request: Request, call_next):
        ip = request.client.host if request.client else ""
        if is_local(ip):
            return await call_next(request)
        if request.url.path.startswith(DESKTOP_PATHS):
            return JSONResponse({"detail": "Only for this machine"}, status_code=403)
        if request.url.path in PUBLIC_PATHS or s.devices.is_valid(bearer_token(request.headers)):
            return await call_next(request)
        return JSONResponse({"detail": "Not paired"}, status_code=401)

    @app.get("/discover")
    async def discover_endpoint():
        """Identifies this server to the apps' LAN scan."""
        return {"service": "saambha", "name": s.hostname}

    @app.post("/pair")
    async def pair_endpoint(body: PairRequest, request: Request):
        """Asks on the desktop whether to allow this device; returns a token if allowed.

        Blocks until answered (up to ~60s).
        """
        ip = request.client.host if request.client else ""
        try:
            token = await s.pairing.request(ip, body.name.strip())
        except RuntimeError as e:
            raise HTTPException(429, str(e)) from e
        if not token:
            raise HTTPException(403, "Denied on the desktop")
        return {"token": token, "name": s.hostname}

    # Transcription and the agent are blocking, so they run in the threadpool to keep
    # the server responsive (e.g. pairing approvals) while a command is in flight.

    @app.post("/listen")
    async def listen_endpoint():
        """The hotkey started recording: cut off the narration and show that it's listening."""
        s.speaker.stop()
        s.status.set_listening(True)
        return {"ok": True}

    @app.delete("/listen")
    async def stop_listening_endpoint():
        """The hotkey stopped recording without anything to send."""
        s.status.set_listening(False)
        return {"ok": True}

    @app.post("/voice")
    async def voice_endpoint(audio: UploadFile = File(...)):
        s.status.set_listening(False)
        s.speaker.stop()
        suffix = os.path.splitext(audio.filename or ".wav")[1] or ".wav"
        data = await audio.read()
        log.info(f"VOICE upload: {len(data)} bytes ({suffix})")
        with tempfile.NamedTemporaryFile(suffix=suffix, delete=False) as f:
            f.write(data)
            tmp_path = f.name
        try:
            try:
                text = await run_in_threadpool(s.transcriber.transcribe, tmp_path)
            except Exception as e:
                log.warning(f"Could not decode audio ({len(data)} bytes): {e}")
                raise HTTPException(400, "Could not read the recording") from e
            if not text:
                raise HTTPException(400, "Could not transcribe audio")
            return await run_in_threadpool(run_command, text)
        finally:
            os.unlink(tmp_path)

    @app.post("/command")
    async def command_endpoint(body: CommandRequest):
        """Run a text command from a client that does its own speech recognition (the apps)."""
        text = body.text.strip()
        if not text:
            raise HTTPException(400, "Empty command")
        s.speaker.stop()
        return await run_in_threadpool(run_command, text, body.speak == "desktop")

    @app.post("/reset")
    async def reset_endpoint():
        s.assistant.reset()
        s.history.start_conversation()
        return {"ok": True, "message": "Session reset"}

    @app.get("/desktop/status")
    async def desktop_status_endpoint(after: int | None = None):
        """What Saambha is doing; with [after], once it's changed since that version (or a while)."""
        return s.status.snapshot() if after is None else await s.status.next(after)

    @app.get("/desktop/history")
    async def desktop_history_endpoint(limit: int = 50):
        """The last [limit] commands and replies, and new conversations, oldest first."""
        return [history.to_json(entry) for entry in s.history.recent(limit)]

    @app.get("/desktop/log")
    async def desktop_log_endpoint(offset: int | None = None):
        """The server log from [offset] on (without: its end), and the offset to ask for next."""
        text, end = s.log.read(offset)
        return {"text": text, "offset": end}

    @app.post("/desktop/pairing")
    async def desktop_pairing_endpoint(body: PairingAnswer):
        """Allows or denies the device waiting to be allowed."""
        if not s.status.answer(body.allow):
            raise HTTPException(409, "No device is waiting")
        return {"ok": True}

    @app.post("/desktop/speech/stop")
    async def desktop_stop_speech_endpoint():
        s.speaker.stop()
        return {"ok": True}

    @app.get("/status")
    async def status_endpoint():
        return {"ok": True, "has_session": s.assistant.has_session()}

    return app
