import logging
import time
from collections.abc import Callable
from pathlib import Path

log = logging.getLogger(__name__)


class SessionStore:
    """The agent session to resume, forgotten after [ttl] seconds without use."""

    def __init__(self, path: Path, ttl: float, clock: Callable[[], float] = time.time):
        self.path = path
        self.ttl = ttl
        self.clock = clock

    def load(self) -> str | None:
        if not self.path.exists():
            return None
        if self.clock() - self.path.stat().st_mtime > self.ttl:
            log.info(f"Session expired after {self.ttl}s, starting new")
            self.clear()
            return None
        return self.path.read_text().strip() or None

    def save(self, session_id: str):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(session_id)

    def clear(self):
        self.path.unlink(missing_ok=True)
