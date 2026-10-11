"""The real pi CLI against a fake model server (tests/fakes/llm_server.py): checks PiHarness
against the events pi actually prints, and that the workspace (prompt, skills, guard) works in pi.

Skipped when pi isn't installed.
"""
import json
import shutil
from pathlib import Path

import pytest

from conftest import PI_GUARD, SYSTEM_PROMPT
from fakes.llm_server import start as start_fake_model
from saambha.agent.pi import PiHarness, with_ollama_model
from saambha.agent.sessions import SessionStore

PI = shutil.which("pi") or str(Path.home() / ".local" / "bin" / "pi")
pytestmark = pytest.mark.skipif(not Path(PI).exists(), reason="pi isn't installed")


@pytest.fixture
def model(fakes, tmp_path, monkeypatch):
    """Points pi (via its own config dir) at the fake model server, set up as Saambha sets up an
    Ollama model; returns its request log."""
    server = start_fake_model(fakes.state)
    agent_dir = tmp_path / "pi-agent"
    agent_dir.mkdir()
    (agent_dir / "models.json").write_text(json.dumps(
        with_ollama_model({}, f"http://127.0.0.1:{server.server_address[1]}", "fake-model", 16384)))
    monkeypatch.setenv("PI_CODING_AGENT_DIR", str(agent_dir))
    monkeypatch.setenv("PI_OFFLINE", "1")
    yield lambda: [json.loads(line) for line in fakes.read("llm_requests.jsonl").splitlines()]
    server.shutdown()


@pytest.fixture
def pi(model, workspace, tmp_path):
    return PiHarness(
        binary=PI,
        workspace=workspace,
        system_prompt_file=SYSTEM_PROMPT,
        model="ollama/fake-model",
        effort="off",
        sessions=SessionStore(tmp_path / "session_id", ttl=3600),
        timeout=60,
        prompt_vars={},
        skills_dir=workspace / "skills",
        extensions=[PI_GUARD],
        session_dir=tmp_path / "pi-sessions",
    )


def tool_results(request) -> list[str]:
    return [json.dumps(m["content"]) for m in request["messages"] if m["role"] == "tool"]


def test_reply(pi):
    assert pi.ask("hello") == "You said: hello"


@pytest.mark.parametrize("effort, sent", [("off", "none"), ("high", "high")])
def test_model_is_told_how_hard_to_think(pi, model, effort, sent):
    pi.effort = effort

    pi.ask("hello")

    assert model()[0]["reasoning_effort"] == sent


def test_model_gets_saambhas_prompt_and_the_skills(pi, model):
    pi.ask("hello")

    system = json.dumps(model()[0]["messages"][0])
    assert "You are Saambha" in system
    assert "spotify" in system and "alarms" in system
    assert [tool["function"]["name"] for tool in model()[0]["tools"]][:2] == ["read", "bash"]


def test_using_a_skill(pi, model):
    events = []

    reply = pi.ask("play some music", on_event=lambda kind, detail: events.append((kind, detail)))

    assert reply == "Playing music"
    assert ("tool_use", "skill spotify") in events
    assert "playerctl" in tool_results(model()[-1])[0]  # the model read the skill's SKILL.md


def test_follow_up_continues_the_conversation(pi, model):
    pi.ask("first")
    pi.ask("second")

    roles_and_text = [(m["role"], json.dumps(m.get("content"))) for m in model()[-1]["messages"]]
    assert any(role == "user" and "first" in text for role, text in roles_and_text)
    assert any(role == "assistant" and "You said: first" in text for role, text in roles_and_text)


def test_guard_blocks_sudo(pi, model):
    pi.ask("run sudo ls")

    assert "sudo is not allowed" in tool_results(model()[-1])[0]


def test_guard_blocks_rm_rf(pi, model, tmp_path):
    keep = tmp_path / "keep"
    keep.mkdir()

    pi.ask(f"run rm -rf {keep}")

    assert "rm -rf is not allowed" in tool_results(model()[-1])[0]
    assert keep.exists()


def test_other_commands_run(pi, model, tmp_path):
    marker = tmp_path / "ran"

    pi.ask(f"run touch {marker}")

    assert marker.exists()
