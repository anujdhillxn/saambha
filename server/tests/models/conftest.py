"""Runs each models test once per model and prints how they compare at the end."""
import json
import os
import re
import shutil
import statistics
import time
from dataclasses import dataclass, field, replace
from pathlib import Path

import pytest

from saambha.agent.base import Harness
from saambha.main import build_harness
from saambha.settings import ROOT, Settings, load_env_file

SAMPLE_SKILLS = Path(__file__).parents[1] / "fixtures" / "skills"
BENCH_SKILLS = Path(__file__).parent / "skills"
FORMATTING = re.compile(r"\*\*|`|^#|^\s*[-*] |^\s*\d+\. |[\U0001F300-\U0001FAFF☀-➿]", re.M)
RESULTS = pytest.StashKey[list]()


def user_settings() -> Settings:
    """The user's settings, without loading server/.env into this process."""
    environ = dict(os.environ)
    load_env_file(ROOT / ".env", environ)
    return Settings.default(environ=environ)


def pytest_generate_tests(metafunc):
    if "model" not in metafunc.fixturenames:
        return
    chosen = metafunc.config.getoption("models")
    if chosen:
        models = [tuple(pair.split(":", 1)) for pair in chosen.split(",")]
    else:
        settings = user_settings()
        models = [("claude", settings.claude_model), ("pi", settings.pi_model)]
    metafunc.parametrize("model", models, ids=[f"{harness}:{name}" for harness, name in models],
                         scope="module")  # one model after another, so Ollama loads each once


@dataclass
class Reply:
    said: str
    text: str
    seconds: float


@dataclass
class Agent:
    """A harness that remembers each reply and how long it took."""
    harness: Harness
    replies: list[Reply] = field(default_factory=list)

    def ask(self, text: str) -> str:
        start = time.monotonic()
        reply = self.harness.ask(text)
        self.replies.append(Reply(text, reply, time.monotonic() - start))
        return reply


@pytest.fixture(scope="session")
def bench_workspace(tmp_path_factory) -> Path:
    """The real prompt and guards, with the sample skills and the benchmark's own."""
    workspace = tmp_path_factory.mktemp("workspace")
    shutil.copy(ROOT / "system-prompt.md", workspace)
    for guard in (".claude/settings.json", ".pi/guard.ts"):
        (workspace / guard).parent.mkdir()
        shutil.copy(ROOT / guard, workspace / guard)
    shutil.copytree(SAMPLE_SKILLS, workspace / "skills")
    shutil.copytree(BENCH_SKILLS, workspace / "skills", dirs_exist_ok=True)
    return workspace


@pytest.fixture(scope="session")
def warmed() -> set:
    return set()


@pytest.fixture
def agent(model, bench_workspace, warmed, tmp_path, monkeypatch) -> Agent:
    """[model] in a fresh session, already loaded so the first command isn't timed loading it."""
    harness_name, model_name = model
    settings = replace(user_settings(), harness=harness_name, workspace=bench_workspace,
                       skills_dir=bench_workspace / "skills", session_file=tmp_path / "session_id",
                       pi_session_dir=tmp_path / "pi-sessions",
                       **{f"{harness_name}_model": model_name})
    if harness_name == "pi" and model_name.startswith("ollama/"):
        # pi only knows the Ollama models listed in its config, so give it one listing this model.
        agent_dir = tmp_path / "pi-agent"
        agent_dir.mkdir()
        (agent_dir / "models.json").write_text(json.dumps({"providers": {"ollama": {
            "baseUrl": f"{settings.ollama_url}/v1", "api": "openai-completions", "apiKey": "ollama",
            "models": [{"id": model_name.removeprefix("ollama/"), "contextWindow": settings.ollama_context}],
        }}}))
        monkeypatch.setenv("PI_CODING_AGENT_DIR", str(agent_dir))
    harness = build_harness(settings)
    if model not in warmed:
        harness.ask("Say OK.")
        harness.reset()
        warmed.add(model)
    return Agent(harness)


@pytest.hookimpl(wrapper=True)
def pytest_runtest_makereport(item, call):
    report = yield
    if call.when == "call" and "agent" in item.funcargs:
        item.config.stash.setdefault(RESULTS, []).append(
            (item.originalname.removeprefix("test_"), item.callspec.id, report.passed, item.funcargs["agent"].replies))
    return report


def pytest_terminal_summary(terminalreporter, config):
    results = config.stash.get(RESULTS, [])
    if not results:
        return
    models = list(dict.fromkeys(model for _, model, _, _ in results))
    tasks = list(dict.fromkeys(task for task, _, _, _ in results))
    cell = {(task, model): (passed, replies) for task, model, passed, replies in results}
    width = max(len(task) for task in tasks + ["plain speech"]) + 2
    columns = [max(len(model), 12) + 2 for model in models]

    def row(name, values):
        terminalreporter.write_line(name.ljust(width) + "".join(v.ljust(c) for v, c in zip(values, columns, strict=True)))

    terminalreporter.section("models")
    row("", models)
    for task in tasks:
        row(task, [f"{'pass' if passed else 'FAIL'} {sum(r.seconds for r in replies):5.1f}s"
                   for passed, replies in (cell.get((task, model), (False, [])) for model in models)])
    summary = {"passed": [], "plain speech": [], "median reply": [], "total": []}
    for model in models:
        mine = [cell[(task, model)] for task in tasks if (task, model) in cell]
        replies = [reply for _, rs in mine for reply in rs]
        summary["passed"].append(f"{sum(passed for passed, _ in mine)}/{len(mine)}")
        summary["plain speech"].append(f"{sum(not FORMATTING.search(r.text) for r in replies)}/{len(replies)}")
        summary["median reply"].append(f"{statistics.median(r.seconds for r in replies):.1f}s" if replies else "-")
        summary["total"].append(f"{sum(r.seconds for r in replies):.1f}s")
    for name, values in summary.items():
        row(name, values)
    if config.getoption("verbose") > 0:
        for _, model, _, replies in results:
            for reply in replies:
                terminalreporter.write_line(f"{model} | {reply.said} -> {reply.text!r}")
