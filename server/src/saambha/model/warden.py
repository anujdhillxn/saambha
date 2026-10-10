"""Keeps the local model loaded, except while something heavy runs (to free the GPU and RAM for it)."""
import logging
import threading
import time
from collections.abc import Callable
from typing import Protocol

log = logging.getLogger(__name__)


class Ollama(Protocol):
    def loaded_models(self) -> list[str]: ...
    def load(self, model: str): ...
    def unload(self, model: str): ...


class ModelWarden:
    """Every [interval] seconds: while [heavy_task] reports something, unload [model] (unless a
    command is using it, see [busy]); once nothing heavy has run for [resume_after] seconds, load
    it again and keep it loaded. Says through [on_cold] why the model isn't loaded, or None once it is."""

    def __init__(self, ollama: Ollama, model: str, heavy_task: Callable[[], str | None],
                 busy: Callable[[], bool], on_cold: Callable[[str | None], None], interval: float,
                 resume_after: float, clock: Callable[[], float] = time.monotonic):
        self.ollama = ollama
        self.model = model
        self.heavy_task = heavy_task
        self.busy = busy
        self.on_cold = on_cold
        self.interval = interval
        self.resume_after = resume_after
        self.clock = clock
        self.cold: str | None = None
        self._last_heavy = float("-inf")
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def check(self):
        """One look at what the user is doing, and the model loaded or unloaded to match."""
        try:
            heavy = self.heavy_task()
            if heavy:
                self._last_heavy = self.clock()
                if self.busy():
                    return  # a command is running: leave the model alone until it's done
                if self.model in self.ollama.loaded_models():
                    self.ollama.unload(self.model)
                self._set_cold(f"Paused: {heavy}")
            elif self.clock() - self._last_heavy >= self.resume_after:
                if self.model not in self.ollama.loaded_models():
                    self._set_cold("Loading the model")
                # Every time, not only when it wasn't loaded: commands load the model with Ollama's
                # default keep-alive, which would unload it again after a few idle minutes.
                self.ollama.load(self.model)
                self._set_cold(None)
        except Exception as e:
            self._set_cold("Ollama isn't responding", detail=str(e))

    def _set_cold(self, reason: str | None, detail: str = ""):
        if reason == self.cold:
            return
        if detail:
            log.warning(f"{self.model}: {reason} ({detail})")
        else:
            log.info(f"{self.model}: {reason or 'loaded'}")
        self.cold = reason
        self.on_cold(reason)

    def start(self):
        self._thread = threading.Thread(target=self._run, name="model-warden", daemon=True)
        self._thread.start()

    def stop(self):
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=5)

    def _run(self):
        while not self._stop.is_set():
            self.check()
            self._stop.wait(self.interval)
