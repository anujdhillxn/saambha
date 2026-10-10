"""Runs an agent CLI that streams JSON events on stdout, with a hard timeout."""
import json
import logging
import subprocess
import tempfile
import threading
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

log = logging.getLogger(__name__)


@dataclass
class RunResult:
    returncode: int
    stderr: str
    timed_out: bool


def run_json_events(cmd: list[str], cwd: Path, timeout: float, on_event: Callable[[dict], None]) -> RunResult:
    """Runs [cmd] in [cwd], calling [on_event] with each JSON line it prints (other lines are
    skipped). Kills it after [timeout] seconds."""
    with tempfile.TemporaryFile(mode="w+") as stderr:  # a file, so a chatty stderr can't block the agent
        proc = subprocess.Popen(cmd, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=stderr, text=True)

        # Reading stdout blocks until the agent exits, so the timeout has to kill it from outside.
        timed_out = threading.Event()

        def kill():
            timed_out.set()
            proc.kill()

        timer = threading.Timer(timeout, kill)
        timer.start()
        try:
            for line in proc.stdout:
                line = line.strip()
                if not line:
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                log.debug(f"STREAM raw: {line[:500]}")
                on_event(event)
            proc.wait()
        finally:
            timer.cancel()

        stderr.seek(0)
        return RunResult(proc.returncode, stderr.read().strip(), timed_out.is_set())
