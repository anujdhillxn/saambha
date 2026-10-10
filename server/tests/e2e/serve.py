"""Runs the real server for the e2e tests: the real wiring (main.build_services), with files
under $SAAMBHA_E2E_DIR and claude/pi/piper/aplay replaced by the fakes in tests/fakes.

Usage: serve.py <port> <harness>
"""
import dataclasses
import os
import sys
from pathlib import Path

import uvicorn

from saambha.main import build_services
from saambha.server.app import create_app
from saambha.settings import Settings

fakes = Path(__file__).resolve().parents[1] / "fakes"
work = Path(os.environ["SAAMBHA_E2E_DIR"])

settings = dataclasses.replace(
    Settings.default(),
    port=int(sys.argv[1]),
    workspace=work,
    skills_dir=work / "skills",
    harness=sys.argv[2],
    cert_file=work / "cert.pem",
    key_file=work / "key.pem",
    devices_file=work / "devices.json",
    session_file=work / "session_id",
    history_file=work / "history.jsonl",
    log_file=work / "server.log",
    claude_bin=str(fakes / "claude"),
    pi_bin=str(fakes / "pi"),
    pi_session_dir=work / "pi-sessions",
    piper_bin=str(fakes / "piper"),
    piper_voice="test-voice",
    piper_voices_dir=work,  # test-voice.onnx exists there, so nothing is downloaded
    aplay_bin=str(fakes / "aplay"),
)
uvicorn.run(create_app(build_services(settings)), host="127.0.0.1", port=settings.port, log_level="warning",
            ssl_keyfile=str(settings.key_file), ssl_certfile=str(settings.cert_file),
            timeout_graceful_shutdown=settings.shutdown_wait)
