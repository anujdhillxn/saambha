import logging
import threading
from abc import ABC, abstractmethod
from collections.abc import Callable, Mapping
from pathlib import Path
from string import Template

from saambha.agent.process import RunResult, run_json_events
from saambha.agent.sessions import SessionStore

log = logging.getLogger(__name__)

EventHandler = Callable[[str, str], None]


class Harness(ABC):
    """An agent CLI that runs commands in the workspace, one at a time, continuing its session
    across commands until the session expires or is reset.

    Subclasses build the command line and read the CLI's JSON events in _ask().
    """

    name: str  # for logs, e.g. "pi"
    TIMEOUT_REPLY = "Sorry, that took too long. Please try again."

    def __init__(self, binary: str, workspace: Path, system_prompt_file: Path, model: str,
                 sessions: SessionStore, timeout: float, prompt_vars: Mapping[str, str]):
        self.binary = binary
        self.workspace = workspace
        self.system_prompt_file = system_prompt_file
        self.prompt_vars = prompt_vars
        self.model = model
        self.sessions = sessions
        self.timeout = timeout
        # One command at a time: concurrent runs would write to the same session.
        self._lock = threading.Lock()

    def ask(self, text: str, on_event: EventHandler | None = None) -> str:
        """Runs [text] as a command and returns the agent's reply, calling
        on_event("text" | "tool_use", detail) as the agent works."""
        with self._lock:
            return self._ask(text, on_event or (lambda kind, detail: None))

    @property
    def busy(self) -> bool:
        """Whether a command is running right now."""
        return self._lock.locked()

    def has_session(self) -> bool:
        return self.sessions.load() is not None

    def reset(self):
        self.sessions.clear()

    @abstractmethod
    def _ask(self, text: str, on_event: EventHandler) -> str: ...

    @property
    def system_prompt(self) -> str:
        """The prompt file (re-read each time, so edits apply at once) with $names from prompt_vars filled in."""
        return Template(self.system_prompt_file.read_text().strip()).safe_substitute(self.prompt_vars)

    def _run(self, cmd: list[str], handle: Callable[[dict], None]) -> RunResult:
        """Runs [cmd] in the workspace, passing each JSON event to [handle]."""
        run = run_json_events(cmd, self.workspace, self.timeout, handle)
        if run.timed_out:
            log.error(f"{self.name} timed out after {self.timeout}s")
        elif run.returncode != 0:
            log.error(f"{self.name} exited {run.returncode}: {run.stderr}")
        return run
