"""Speaking replies on the desktop (piper | aplay)."""
import contextlib
import logging
import os
import shlex
import signal
import subprocess
import threading
from collections.abc import Callable
from pathlib import Path

log = logging.getLogger(__name__)


def piper_voice_path(voices_dir: Path, voice: str) -> Path:
    return voices_dir / f"{voice}.onnx"


def ensure_piper_voice(voices_dir: Path, voice: str, voices_url: str):
    """Downloads the piper voice (e.g. en_US-lessac-medium) from [voices_url] if it isn't there yet."""
    if piper_voice_path(voices_dir, voice).exists():
        return
    voices_dir.mkdir(parents=True, exist_ok=True)
    # en_US-lessac-medium -> en/en_US/lessac/medium
    lang_region, name, quality = voice.split("-")
    base = f"{voices_url}/{lang_region.split('_')[0]}/{lang_region}/{name}/{quality}/{voice}"
    log.info(f"Downloading piper voice {voice}...")
    for ext in (".onnx", ".onnx.json"):
        subprocess.run(["curl", "-L", "-o", str(voices_dir / f"{voice}{ext}"), f"{base}{ext}"], check=True)
    log.info("Piper voice downloaded")


class Narrator:
    """Speaks replies on the desktop, one at a time, and says through [on_speaking] when it starts and stops.

    Each narration is a piper | aplay pipeline in its own process group, so stopping it stops both.
    """

    def __init__(self, piper: str, voice_path: Path, on_speaking: Callable[[bool], None], aplay: str = "aplay"):
        self.pipeline = (f"{shlex.quote(piper)} -m {shlex.quote(str(voice_path))} --output-raw"
                         f" | {shlex.quote(aplay)} -r 22050 -f S16_LE -q")
        self.on_speaking = on_speaking
        self._speech: subprocess.Popen | None = None
        self._lock = threading.Lock()

    def speak(self, text: str):
        """Speaks [text], replacing any narration already playing."""
        with self._lock:
            self._stop_locked()
            # The text goes in on stdin, so the shell never sees it.
            speech = subprocess.Popen(self.pipeline, shell=True, start_new_session=True,
                                      stdin=subprocess.PIPE, text=True)
            speech.stdin.write(text)
            speech.stdin.close()
            self._speech = speech
            self.on_speaking(True)
        threading.Thread(target=self._finish, args=(speech,), daemon=True).start()

    def stop(self):
        with self._lock:
            self._stop_locked()

    def _finish(self, speech: subprocess.Popen):
        speech.wait()
        with self._lock:
            if self._speech is speech:  # not already stopped or replaced
                self._speech = None
                self.on_speaking(False)

    def _stop_locked(self):
        if not self._speech:
            return
        if self._speech.poll() is None:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(self._speech.pid, signal.SIGTERM)
            log.info("Stopped narration")
        self._speech = None
        self.on_speaking(False)
