"""What Saambha is doing, for the desktop app (the dot and its chat panel) to show, and the
question of whether to allow a new device, which the desktop app answers. The desktop app waits
for each change (next) rather than asking over and over, so neither does anything meanwhile."""
import asyncio
import concurrent.futures
import contextlib
import threading
from dataclasses import asdict, dataclass, replace


@dataclass(frozen=True)
class Activity:
    model_cold: str | None = None  # why the local model isn't loaded, e.g. "Loading the model"
    listening: bool = False
    task: str | None = None  # what the agent is doing, e.g. "Running skill spotify"
    speaking: bool = False
    pairing: str | None = None  # the device waiting to be allowed, e.g. "Pixel Watch 4 (192.168.0.12)"

    @property
    def look(self) -> tuple[str, str]:
        """The state the dot shows (its colour) and the status line: the most pressing thing going on."""
        if self.pairing:
            return "asking", f"{self.pairing} wants to connect"
        if self.listening:
            return "listening", "Listening"
        if self.task:
            return "working", self.task
        if self.speaking:
            return "working", "Speaking"
        if self.model_cold:
            return "cold", self.model_cold
        return "ready", "Ready"


class Status:
    """Thread-safe: set from the agent's, the narrator's and the model warden's threads."""

    def __init__(self, approval_timeout: float, change_wait: float):
        self.approval_timeout = approval_timeout
        self.change_wait = change_wait
        self._activity = Activity()
        self._version = 0  # counts the changes
        self._changed = concurrent.futures.Future()  # done at the next change
        self._answer: concurrent.futures.Future | None = None
        self._lock = threading.Lock()

    @property
    def activity(self) -> Activity:
        with self._lock:
            return self._activity

    def snapshot(self) -> dict:
        with self._lock:
            activity, version = self._activity, self._version
        state, line = activity.look
        return {"state": state, "line": line, **asdict(activity), "version": version}

    async def next(self, after: int) -> dict:
        """The snapshot once it's changed from version [after] (at once if it has, as after a restart),
        else as it is after the wait."""
        with self._lock:
            changed = self._changed if self._version == after else None
        if changed:
            with contextlib.suppress(TimeoutError):
                # Shielded: the waiters share the future, which a timed-out one mustn't cancel.
                await asyncio.wait_for(asyncio.shield(asyncio.wrap_future(changed)), self.change_wait)
        return self.snapshot()

    def set_model_cold(self, reason: str | None):
        self._update(model_cold=reason)

    def set_listening(self, listening: bool):
        self._update(listening=listening)

    def set_task(self, task: str | None):
        self._update(task=task)

    def set_speaking(self, speaking: bool):
        self._update(speaking=speaking)

    async def ask(self, ip: str, device_name: str) -> bool:
        """Asks the desktop app whether to allow a new device: whether it was allowed in time."""
        answer = concurrent.futures.Future()
        with self._lock:
            self._answer = answer
        self._update(pairing=f"{device_name} ({ip})")
        try:
            return await asyncio.wait_for(asyncio.wrap_future(answer), self.approval_timeout)
        except TimeoutError:
            return False
        finally:
            with self._lock:
                self._answer = None
            self._update(pairing=None)

    def answer(self, allowed: bool) -> bool:
        """Answers the device waiting to be allowed; False if none is."""
        with self._lock:
            answer = self._answer
        if not answer:
            return False
        with contextlib.suppress(concurrent.futures.InvalidStateError):  # timed out just now
            answer.set_result(allowed)
        return True

    def _update(self, **changes):
        with self._lock:
            self._activity = replace(self._activity, **changes)
            self._version += 1
            changed, self._changed = self._changed, concurrent.futures.Future()
        changed.set_result(None)
