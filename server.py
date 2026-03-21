import os
import tempfile
import subprocess
import logging
from contextlib import asynccontextmanager
from pathlib import Path


def _load_env():
    env_file = Path(__file__).parent / ".env"
    if not env_file.exists():
        return
    for line in env_file.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, _, value = line.partition("=")
            os.environ.setdefault(key.strip(), value.strip())

_load_env()

from fastapi import FastAPI, UploadFile, File, HTTPException
from pydantic import BaseModel

import session
from transcribe import transcribe

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
log = logging.getLogger("voice-ai")

PORT = int(os.environ.get("PORT", 8765))


def notify(title: str, body: str = ""):
    env = {**os.environ, "DISPLAY": ":1"}
    text = f"{title}: {body}" if body else title
    subprocess.Popen(["zenity", "--notification", f"--text={text}"], env=env)


def speak(text: str):
    subprocess.Popen(["espeak-ng", text])


def run_command(text: str) -> dict:
    log.info(f'INPUT: "{text}"')
    response_text = session.ask(text)
    log.info(f'RESPONSE: {response_text!r}')
    if response_text:
        notify(response_text)
        #speak(response_text)
    return {"input": text, "text": response_text}


@asynccontextmanager
async def lifespan(app: FastAPI):
    print(f"Voice AI server running on http://0.0.0.0:{PORT}")
    yield


app = FastAPI(lifespan=lifespan)


class TextCommand(BaseModel):
    text: str


@app.post("/voice")
async def voice_endpoint(audio: UploadFile = File(...)):
    suffix = os.path.splitext(audio.filename or ".wav")[1] or ".wav"
    with tempfile.NamedTemporaryFile(suffix=suffix, delete=False) as f:
        f.write(await audio.read())
        tmp_path = f.name
    try:
        text = transcribe(tmp_path)
        if not text:
            raise HTTPException(400, "Could not transcribe audio")
        return run_command(text)
    finally:
        os.unlink(tmp_path)


@app.post("/text")
async def text_endpoint(body: TextCommand):
    if not body.text.strip():
        raise HTTPException(400, "Empty command")
    return run_command(body.text.strip())


@app.post("/reset")
async def reset_endpoint():
    session.reset()
    return {"ok": True, "message": "Session reset"}


@app.get("/status")
async def status_endpoint():
    return {
        "ok": True,
        "has_session": session._load_session_id() is not None,
    }


from fastapi.staticfiles import StaticFiles
app.mount("/", StaticFiles(directory="static", html=True), name="static")


if __name__ == "__main__":
    import uvicorn
    import pathlib
    _dir = pathlib.Path(__file__).parent
    uvicorn.run("server:app", host="0.0.0.0", port=PORT, reload=True, reload_delay=10.0,
                ssl_keyfile=str(_dir / "key.pem"), ssl_certfile=str(_dir / "cert.pem"))
