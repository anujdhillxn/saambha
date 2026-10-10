import os
import threading

import pytest

from conftest import FAKE_CLAUDE, ROOT, SYSTEM_PROMPT
from saambha.agent.claude_code import ClaudeCodeHarness
from saambha.agent.sessions import SessionStore


def test_returns_the_reply_and_streams_events(claude):
    events = []

    reply = claude.ask("what time is it", on_event=lambda kind, detail: events.append((kind, detail)))

    assert reply == "You said: what time is it"
    assert events == [("tool_use", "skill spotify"), ("text", "You said: what time is it")]


def test_runs_in_the_workspace_with_model_and_system_prompt(claude, fakes, workspace):
    claude.ask("hi")

    [call] = fakes.claude_calls()
    args = call["args"]
    assert call["cwd"] == str(workspace)
    assert args[args.index("--model") + 1] == "test-model"
    assert args[args.index("--system-prompt") + 1] == SYSTEM_PROMPT.read_text().strip()
    assert args[args.index("--allowedTools") + 1:args.index("--allowedTools") + 3] == ["Bash", "Skill"]


def test_skills_are_where_claude_code_looks(claude, workspace):
    assert (workspace / ".claude" / "skills" / "spotify" / "SKILL.md").exists()


def test_the_real_workspace_has_the_guard():
    assert "sudo is not allowed" in (ROOT / ".claude" / "settings.json").read_text()


def test_follow_up_resumes_the_session(claude, fakes):
    claude.ask("first")
    claude.ask("second")

    first, second = fakes.claude_calls()
    assert "--resume" not in first["args"]
    assert second["args"][second["args"].index("--resume") + 1] == "sess-1"
    assert claude.has_session()


def test_reset_forgets_the_session(claude, fakes):
    claude.ask("first")
    claude.reset()
    claude.ask("second")

    assert not any("--resume" in call["args"] for call in fakes.claude_calls())


def test_failed_resume_retries_with_a_new_session(claude, fakes):
    claude.ask("first")
    fakes.set("claude_mode", "fail-resume")

    reply = claude.ask("second")

    assert reply == "You said: second"
    _, failed, retried = fakes.claude_calls()
    assert "--resume" in failed["args"]
    assert "--resume" not in retried["args"]


def test_hung_claude_is_killed_after_the_timeout(claude, fakes):
    claude.timeout = 1
    fakes.set("claude_mode", "hang")
    replies = []

    # In a thread, so a broken timeout fails the test instead of hanging the suite.
    asking = threading.Thread(target=lambda: replies.append(claude.ask("hello")), daemon=True)
    asking.start()
    asking.join(timeout=5)

    assert replies == ["Sorry, that took too long. Please try again."]


def harness_in(workspace, skills_dir):
    return ClaudeCodeHarness(binary=FAKE_CLAUDE, workspace=workspace,
                             system_prompt_file=workspace / "system-prompt.md", model="m",
                             sessions=SessionStore(workspace / "session_id", ttl=3600), timeout=5,
                             prompt_vars={}, skills_dir=skills_dir)


def test_skills_folder_is_linked_where_claude_code_looks(tmp_path):
    skills = tmp_path / "elsewhere" / "skills"

    harness_in(tmp_path, skills)

    link = tmp_path / ".claude" / "skills"
    assert link.is_symlink() and link.resolve() == skills.resolve()
    assert os.readlink(link) == "../elsewhere/skills"  # relative, so the folder can move


def test_link_follows_a_changed_skills_setting(tmp_path):
    harness_in(tmp_path, tmp_path / "old")

    harness_in(tmp_path, tmp_path / "new")

    assert (tmp_path / ".claude" / "skills").resolve() == (tmp_path / "new").resolve()


def test_real_skills_folder_is_not_replaced(tmp_path):
    (tmp_path / ".claude" / "skills").mkdir(parents=True)

    with pytest.raises(RuntimeError, match="is a folder"):
        harness_in(tmp_path, tmp_path / "skills")
