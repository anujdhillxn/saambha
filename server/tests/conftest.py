import json
import shutil
import time
from pathlib import Path

import pytest

from saambha.agent.claude_code import ClaudeCodeHarness
from saambha.agent.pi import PiHarness
from saambha.agent.sessions import SessionStore
from saambha.server.auth import DeviceRegistry
from saambha.settings import ROOT
from saambha.voice.narrator import Narrator

FAKES_DIR = Path(__file__).parent / "fakes"
FAKE_CLAUDE = str(FAKES_DIR / "claude")
FAKE_PI = str(FAKES_DIR / "pi")
FAKE_PIPER = str(FAKES_DIR / "piper")
FAKE_APLAY = str(FAKES_DIR / "aplay")
# The real prompt and pi guard; the skills are samples (the user's own live in a separate repo).
SYSTEM_PROMPT = ROOT / "system-prompt.md"
PI_GUARD = ROOT / ".pi" / "guard.ts"
SAMPLE_SKILLS = Path(__file__).parent / "fixtures" / "skills"


def pytest_addoption(parser):
    parser.addoption("--models", help="for models/: harness:model pairs to compare, comma-separated "
                                      "(default: the configured claude_model and pi_model)")
    parser.addoption("--runs", type=int, default=3, help="for models/: how many times each task runs per model")


class Fakes:
    """State shared with the fake programs in tests/fakes (see each one's header)."""

    def __init__(self, state: Path):
        self.state = state

    def set(self, name: str, value):
        (self.state / name).write_text(str(value))

    def read(self, name: str) -> str:
        path = self.state / name
        return path.read_text() if path.exists() else ""

    def claude_calls(self) -> list[dict]:
        return [json.loads(line) for line in self.read("claude_calls.jsonl").splitlines()]

    def pi_calls(self) -> list[dict]:
        return [json.loads(line) for line in self.read("pi_calls.jsonl").splitlines()]

    def spoken(self) -> str:
        return self.read("spoken.txt")

    def piper_pid(self) -> int:
        return int(self.read("piper.pid"))


@pytest.fixture
def fakes(tmp_path, monkeypatch) -> Fakes:
    """Where the fake programs keep their state for this test."""
    state = tmp_path / "fakes"
    state.mkdir()
    monkeypatch.setenv("SAAMBHA_FAKES_STATE", str(state))
    return Fakes(state)


class Clock:
    """A clock tests can move forward."""

    def __init__(self, now: float = 1_000_000.0):
        self.now = now

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float):
        self.now += seconds


@pytest.fixture
def clock() -> Clock:
    return Clock()


@pytest.fixture
def registry(tmp_path) -> DeviceRegistry:
    return DeviceRegistry(tmp_path / "devices.json")


@pytest.fixture
def workspace(tmp_path) -> Path:
    """A throwaway agent workspace with the sample skills in workspace/skills."""
    workspace = tmp_path / "workspace"
    shutil.copytree(SAMPLE_SKILLS, workspace / "skills")
    return workspace


@pytest.fixture
def claude(fakes, workspace, tmp_path) -> ClaudeCodeHarness:
    return ClaudeCodeHarness(
        binary=FAKE_CLAUDE,
        workspace=workspace,
        system_prompt_file=SYSTEM_PROMPT,
        model="test-model",
        effort="off",
        sessions=SessionStore(tmp_path / "session_id", ttl=3600),
        timeout=5,
        prompt_vars={},
        skills_dir=workspace / "skills",
    )


@pytest.fixture
def pi(fakes, workspace, tmp_path) -> PiHarness:
    ids = iter(f"session-{n}" for n in range(1, 100))
    return PiHarness(
        binary=FAKE_PI,
        workspace=workspace,
        system_prompt_file=SYSTEM_PROMPT,
        model="anthropic/test-model",
        effort="medium",
        sessions=SessionStore(tmp_path / "session_id", ttl=3600),
        timeout=5,
        prompt_vars={},
        skills_dir=workspace / "skills",
        extensions=[PI_GUARD],
        session_dir=tmp_path / "pi-sessions",
        new_session_id=lambda: next(ids),
    )


@pytest.fixture
def narrator(fakes, tmp_path):
    """A narrator whose .speaking records what it said through on_speaking."""
    speaking = []
    narrator = Narrator(piper=FAKE_PIPER, voice_path=tmp_path / "voice.onnx", on_speaking=speaking.append,
                        aplay=FAKE_APLAY)
    narrator.speaking = speaking
    yield narrator
    narrator.stop()


def wait_until(condition, timeout=5.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if condition():
            return True
        time.sleep(0.05)
    return False


def is_running(pid: int) -> bool:
    try:
        with open(f"/proc/{pid}/stat") as f:
            return f.read().split()[2] != "Z"  # a zombie has exited
    except (FileNotFoundError, ProcessLookupError):  # gone, or going as it's read
        return False
