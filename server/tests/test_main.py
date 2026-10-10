import dataclasses

import pytest

from saambha.agent.claude_code import ClaudeCodeHarness
from saambha.agent.pi import PiHarness
from saambha.main import build_harness, build_model_warden, build_services
from saambha.settings import Settings


@pytest.fixture
def settings(tmp_path):
    """Default settings, but with the workspace and skills in tmp_path."""
    (tmp_path / "system-prompt.md").write_text("You are Saambha.")
    return dataclasses.replace(Settings.default(root=tmp_path, environ={}), skills_dir=tmp_path / "skills")


def test_pi_harness(settings):
    harness = build_harness(dataclasses.replace(settings, harness="pi"))

    assert isinstance(harness, PiHarness)
    assert harness.model == settings.pi_model
    assert harness.skills_dir == settings.skills_dir
    assert harness.extensions == [settings.workspace / ".pi" / "guard.ts"]


def test_claude_harness(settings):
    harness = build_harness(dataclasses.replace(settings, harness="claude"))

    assert isinstance(harness, ClaudeCodeHarness)
    assert harness.model == settings.claude_model


def test_both_harnesses_share_the_workspace_and_prompt(settings):
    pi = build_harness(dataclasses.replace(settings, harness="pi"))
    claude = build_harness(dataclasses.replace(settings, harness="claude"))

    assert pi.workspace == claude.workspace == settings.workspace
    assert pi.system_prompt == claude.system_prompt != ""


def test_both_harnesses_use_the_skills_setting(settings, tmp_path):
    skills = tmp_path / "my-skills"
    settings = dataclasses.replace(settings, skills_dir=skills)

    pi = build_harness(dataclasses.replace(settings, harness="pi"))
    claude = build_harness(dataclasses.replace(settings, harness="claude"))

    assert pi.skills_dir == claude.skills_dir == skills
    assert (tmp_path / ".claude" / "skills").resolve() == skills.resolve()


def test_unknown_harness(settings):
    with pytest.raises(ValueError, match="Unknown harness 'codex'"):
        build_harness(dataclasses.replace(settings, harness="codex"))


@pytest.mark.parametrize("harness, model, warden", [
    ("pi", "ollama/gemma4:e4b", True),
    ("pi", "anthropic/claude-haiku-5-5", False),
    ("claude", "ollama/gemma4:e4b", False),
])
def test_model_warden_only_for_a_local_model(settings, harness, model, warden):
    settings = dataclasses.replace(settings, harness=harness, pi_model=model)
    services = build_services(settings)

    built = build_model_warden(settings, services.assistant, services.status)

    assert (built is not None) == warden
    if built:
        assert built.model == "gemma4:e4b"
        assert built.resume_after == settings.heavy_resume_after


def test_the_prompt_names_the_real_workspace_and_skills(settings, tmp_path):
    settings.system_prompt_file.write_text("Work in $workspace, skills in $skills_dir.")

    harness = build_harness(dataclasses.replace(settings, harness="pi"))

    assert harness.system_prompt == f"Work in {tmp_path}, skills in {tmp_path / 'skills'}."
