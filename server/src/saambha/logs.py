"""Logging for the whole server: one format, to the console and (optionally) a rotating log file.

Modules log through `logging.getLogger(__name__)`; only the composition root calls configure_logging.
"""
import logging
import logging.handlers
from pathlib import Path

from saambha.settings import Settings

FORMAT = "%(asctime)s %(levelname)s %(name)s: %(message)s"

# uvicorn's loggers, sent through our handlers instead of uvicorn's own (run uvicorn with log_config=None).
UVICORN_LOGGERS = ("uvicorn", "uvicorn.error", "uvicorn.access")


def configure_logging(settings: Settings, to_file: bool = True):
    """Sets up the root logger. Safe to call again: it replaces its handlers rather than adding more.

    [to_file]: also write settings.log_file, rotated at settings.log_max_bytes. Only one process
    should do this (rotation isn't safe across processes), so the auto-reload supervisor doesn't.
    """
    handlers: list[logging.Handler] = [logging.StreamHandler()]
    if to_file:
        handlers.append(logging.handlers.RotatingFileHandler(
            settings.log_file, maxBytes=settings.log_max_bytes, backupCount=settings.log_backups,
            encoding="utf-8",
        ))

    root = logging.getLogger()
    for handler in root.handlers[:]:
        root.removeHandler(handler)
        handler.close()
    formatter = logging.Formatter(FORMAT)
    for handler in handlers:
        handler.setFormatter(formatter)
        root.addHandler(handler)
    root.setLevel(settings.log_level)

    for name in UVICORN_LOGGERS:
        uvicorn_logger = logging.getLogger(name)
        uvicorn_logger.handlers.clear()
        uvicorn_logger.propagate = True


class LogFile:
    """The log file, read a piece at a time by the desktop app's log window as it grows."""

    def __init__(self, path: Path, backlog: int):
        self.path = path
        self.backlog = backlog

    def read(self, offset: int | None) -> tuple[str, int]:
        """What's been written from [offset] on, and the offset to carry on from. With no offset:
        the last [backlog] bytes, from a line start. An offset past the end means the log was
        rotated since, so it starts over from the new one's start."""
        try:
            size = self.path.stat().st_size
        except FileNotFoundError:
            return "", 0
        if offset is None:
            start = max(0, size - self.backlog)
        elif offset > size:
            start = 0
        else:
            start = offset
        with self.path.open("rb") as f:
            f.seek(start)
            data = f.read(size - start)
        if offset is None and start > 0:
            data = data.partition(b"\n")[2]  # the first line is cut off
        return data.decode(errors="replace"), size
