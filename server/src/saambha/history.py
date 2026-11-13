"""The chat history: each command and Saambha's reply, and where a new conversation started, one
JSON object per line."""
import json
import logging
import os
import time
from collections.abc import Callable
from dataclasses import asdict, dataclass
from pathlib import Path

log = logging.getLogger(__name__)


@dataclass(frozen=True)
class Exchange:
    time: float
    said: str
    reply: str  # what was spoken
    details: str = ""  # only shown in the chat


@dataclass(frozen=True)
class NewConversation:
    """The agent forgot what came before."""
    time: float


Entry = Exchange | NewConversation


def to_json(entry: Entry) -> dict:
    return asdict(entry) | ({"new_conversation": True} if isinstance(entry, NewConversation) else {})


def from_json(fields: dict) -> Entry:
    return NewConversation(fields["time"]) if fields.get("new_conversation") else Exchange(**fields)


class ChatHistory:
    def __init__(self, path: Path, clock: Callable[[], float] = time.time):
        self.path = path
        self.clock = clock

    def add(self, said: str, reply: str, details: str):
        self._write(Exchange(self.clock(), said, reply, details))

    def start_conversation(self):
        """Marks a new conversation, after the last exchange (if any, and if not marked already)."""
        if any(isinstance(entry, Exchange) for entry in self.recent(1)):
            self._write(NewConversation(self.clock()))

    def recent(self, limit: int) -> list[Entry]:
        """The last [limit] entries, oldest first."""
        if not self.path.exists():
            return []
        lines = self.path.read_text(encoding="utf-8").splitlines()[-limit:]
        return [entry for line in lines if (entry := _read(line)) is not None]

    def _write(self, entry: Entry):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with self.path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(to_json(entry)) + "\n")
            f.flush()
            os.fsync(f.fileno())  # on disk before, say, the command that shuts the PC down


def _read(line: str) -> Entry | None:
    """The entry on [line], or None if it can't be read. A crash mid-write can leave a line padded
    with NUL bytes, ahead of the entry."""
    try:
        return from_json(json.loads(line.strip("\0")))
    except (ValueError, TypeError, KeyError):
        log.warning("Leaving out a line of the chat history that can't be read: %r", line[:200])
        return None
