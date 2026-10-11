import threading
import time

from saambha.agent.base import Harness
from saambha.agent.sessions import SessionStore


class EchoHarness(Harness):
    """A minimal harness: shows what the base class gives every harness."""
    name = "echo"

    def __init__(self, tmp_path, **kwargs):
        prompt = tmp_path / "system-prompt.md"
        prompt.write_text("  Be brief.  \n")
        super().__init__(binary="echo", workspace=tmp_path, system_prompt_file=prompt, model="m", effort="off",
                         sessions=SessionStore(tmp_path / "session_id", ttl=3600), timeout=5,
                         prompt_vars=kwargs.get("prompt_vars", {}))
        self.running = 0
        self.overlapped = False

    def _ask(self, text, on_event):
        self.running += 1
        self.overlapped |= self.running > 1
        time.sleep(0.05)
        on_event("text", text)
        self.sessions.save("s")
        self.running -= 1
        return f"{self.system_prompt} {text}"


def test_commands_run_one_at_a_time(tmp_path):
    harness = EchoHarness(tmp_path)

    threads = [threading.Thread(target=harness.ask, args=(f"q{i}",)) for i in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    assert not harness.overlapped


def test_on_event_is_optional(tmp_path):
    assert EchoHarness(tmp_path).ask("hi") == "Be brief. hi"


def test_events_reach_the_caller(tmp_path):
    events = []

    EchoHarness(tmp_path).ask("hi", on_event=lambda kind, detail: events.append((kind, detail)))

    assert events == [("text", "hi")]


def test_session_status_and_reset(tmp_path):
    harness = EchoHarness(tmp_path)
    assert not harness.has_session()

    harness.ask("hi")
    assert harness.has_session()

    harness.reset()
    assert not harness.has_session()


def test_runs_the_cli_in_the_workspace(tmp_path):
    harness = EchoHarness(tmp_path)
    events = []

    run = harness._run(["sh", "-c", 'pwd; echo \'{"type": "x"}\'; echo not-json'], events.append)

    assert run.returncode == 0 and not run.timed_out
    assert events == [{"type": "x"}]


def test_stderr_is_kept_for_the_logs(tmp_path):
    run = EchoHarness(tmp_path)._run(["sh", "-c", "echo oops >&2; exit 3"], lambda e: None)

    assert (run.returncode, run.stderr) == (3, "oops")


def test_busy_while_a_command_runs(tmp_path):
    harness = EchoHarness(tmp_path)
    seen = []
    harness._ask = lambda text, on_event: seen.append(harness.busy) or "done"

    harness.ask("hi")

    assert seen == [True]
    assert not harness.busy


def test_prompt_placeholders_are_filled_in(tmp_path):
    harness = EchoHarness(tmp_path, prompt_vars={"skills_dir": "/home/u/skills"})
    harness.system_prompt_file.write_text("Skills: $skills_dir/<name>. Cost: $5, $unknown stays.")

    assert harness.system_prompt == "Skills: /home/u/skills/<name>. Cost: $5, $unknown stays."


def test_prompt_edits_apply_without_a_restart(tmp_path):
    harness = EchoHarness(tmp_path)
    harness.system_prompt_file.write_text("New rule.")

    assert harness.system_prompt == "New rule."
