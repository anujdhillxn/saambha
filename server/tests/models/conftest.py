"""Runs each models test --runs times per model, puts back whatever an agent changed in Saambha's data
folder, and prints how the models compare at the end."""
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
from saambha.agent.pi import with_ollama_model
from saambha.main import build_harness
from saambha.settings import ROOT, Settings, load_env_file

SAMPLE_SKILLS = Path(__file__).parents[1] / "fixtures" / "skills"
BENCH_SKILLS = Path(__file__).parent / "skills"
FORMATTING = re.compile(r"\*\*|`|^#|^\s*[-*] |^\s*\d+\. |[\U0001F300-\U0001FAFF☀-➿]", re.M)
LONG_REPLY = 40  # words: about 15 seconds spoken
RESULTS = pytest.StashKey[list]()
CHANGED = pytest.StashKey[list]()


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
    runs = range(1, metafunc.config.getoption("runs") + 1)
    metafunc.parametrize("run", runs, ids=[f"run{n}" for n in runs])


@dataclass
class Reply:
    said: str
    text: str
    seconds: float


@dataclass
class Agent:
    """A harness that remembers each reply it's asked for and how long it took."""
    harness: Harness
    replies: list[Reply] = field(default_factory=list)

    def ask(self, text: str) -> str:
        start = time.monotonic()
        reply = self.harness.ask(text)
        self.replies.append(Reply(text, reply, time.monotonic() - start))
        return reply

    def chat(self, *texts: str):
        """Says [texts] first, in the same conversation, without counting the replies."""
        for text in texts:
            self.harness.ask(text)


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
def agent(model, run, bench_workspace, warmed, tmp_path, monkeypatch) -> Agent:
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
        (agent_dir / "models.json").write_text(json.dumps(with_ollama_model(
            {}, settings.ollama_url, model_name.removeprefix("ollama/"), settings.ollama_context)))
        monkeypatch.setenv("PI_CODING_AGENT_DIR", str(agent_dir))
    harness = build_harness(settings)
    if model not in warmed:
        harness.ask("Say OK.")
        harness.reset()
        warmed.add(model)
    return Agent(harness)


def snapshot(folder: Path) -> dict[Path, bytes]:
    return {path: path.read_bytes() for path in folder.rglob("*") if path.is_file()}


@pytest.fixture(autouse=True)
def leaves_no_trace(request):
    """Puts back what an agent added or changed in Saambha's data folder (notes, learnings), and
    says whether it did. The live server's history and pi's sessions are left alone."""
    settings = user_settings()
    data = settings.history_file.parent

    def the_servers(path: Path) -> bool:
        return path == settings.history_file or settings.pi_session_dir in path.parents

    before = snapshot(data)
    yield
    after = snapshot(data)
    changed = [path for path in after.keys() | before.keys()
               if not the_servers(path) and after.get(path) != before.get(path)]
    for path in changed:
        if path in before:
            path.write_bytes(before[path])
        else:
            path.unlink()
    request.node.stash[CHANGED] = sorted(str(path) for path in changed)



@pytest.hookimpl(wrapper=True)
def pytest_runtest_makereport(item, call):
    report = yield
    if call.when == "call" and "agent" in item.funcargs:
        model = "{}:{}".format(*item.callspec.params["model"])
        item.config.stash.setdefault(RESULTS, []).append(
            (item.originalname.removeprefix("test_"), model, report.passed, item.funcargs["agent"].replies, item))
    return report


def pytest_terminal_summary(terminalreporter, config):
    results = config.stash.get(RESULTS, [])
    if not results:
        return
    models = list(dict.fromkeys(model for _, model, _, _, _ in results))
    tasks = list(dict.fromkeys(task for task, _, _, _, _ in results))
    runs = {}
    for task, model, passed, replies, _ in results:
        runs.setdefault((task, model), []).append((passed, replies))
    width = max(len(task) for task in tasks + ["left changes behind"]) + 2
    columns = [max(len(model), 14) + 2 for model in models]

    def row(name, values):
        terminalreporter.write_line(name.ljust(width) + "".join(v.ljust(c) for v, c in zip(values, columns, strict=True)))

    def median_seconds(replies):
        return f"{statistics.median(r.seconds for r in replies):.1f}s" if replies else "-"

    terminalreporter.section("models")
    row("", models)
    for task in tasks:
        cells = []
        for model in models:
            done = runs.get((task, model), [])
            passes = sum(passed for passed, _ in done)
            mark = "" if passes == len(done) else "  <"
            cells.append(f"{passes}/{len(done)} {median_seconds([r for _, rs in done for r in rs]):>6}{mark}")
        row(task, cells)
    summary = {"passed": [], "plain speech": [], f"over {LONG_REPLY} words": [], "left changes behind": [],
               "median reply": [], "total": []}
    for model in models:
        mine = [(passed, replies, item) for task, m, passed, replies, item in results if m == model]
        replies = [reply for _, rs, _ in mine for reply in rs]
        summary["passed"].append(f"{sum(passed for passed, _, _ in mine)}/{len(mine)}")
        summary["plain speech"].append(f"{sum(not FORMATTING.search(r.text) for r in replies)}/{len(replies)}")
        summary[f"over {LONG_REPLY} words"].append(f"{sum(len(r.text.split()) > LONG_REPLY for r in replies)}/{len(replies)}")
        summary["left changes behind"].append(f"{sum(bool(item.stash.get(CHANGED, [])) for _, _, item in mine)}/{len(mine)}")
        summary["median reply"].append(median_seconds(replies))
        summary["total"].append(f"{sum(r.seconds for r in replies):.0f}s")
    for name, values in summary.items():
        row(name, values)
    if config.getoption("verbose") > 0:
        for task, model, passed, replies, item in results:
            for reply in replies:
                terminalreporter.write_line(f"{'' if passed else 'FAILED '}{model} {task} | {reply.said} -> {reply.text!r}")
            if changes := item.stash.get(CHANGED, []):
                terminalreporter.write_line(f"{model} {task} | changed (put back): {', '.join(changes)}")
