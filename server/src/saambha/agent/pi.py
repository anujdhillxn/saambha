"""The pi harness (https://pi.dev): `pi --mode json` in the workspace.

pi gets saambha's prompt appended to its own, the skills in skills_dir (listed in its system
prompt; the model reads a skill's SKILL.md when it uses it), and the extensions given (.pi/guard.ts
blocks sudo and rm -rf). Saambha picks the session id, so a follow-up continues the same session.
"""
import copy
import logging
import re
import uuid
from collections.abc import Callable, Mapping
from pathlib import Path

from saambha.agent.base import EventHandler, Harness
from saambha.agent.sessions import SessionStore

log = logging.getLogger(__name__)

SKILL_FILE = re.compile(r"(?:^|/)skills/([^/]+)/SKILL\.md$")


def with_ollama_model(config: dict, url: str, model: str, context: int) -> dict:
    """pi's models.json [config] with Ollama's [model] in it, keeping the other entries.

    It's marked a reasoning model, so pi passes on its thinking level, "off" as Ollama's "none":
    told nothing, models like Gemma 4 think at length anyway.
    """
    config = copy.deepcopy(config)
    provider = config.setdefault("providers", {}).setdefault("ollama", {})
    provider.update(baseUrl=f"{url.rstrip('/')}/v1", api="openai-completions", apiKey="ollama")
    models = [m for m in provider.get("models", []) if m.get("id") != model]
    provider["models"] = models + [{"id": model, "contextWindow": context, "reasoning": True,
                                    "thinkingLevelMap": {"off": "none"}}]
    return config


class PiHarness(Harness):
    name = "pi"
    DONE_REPLY = "Done."  # when the model ran something, then finished without a word
    SILENT_REPLY = "Sorry, I didn't get anywhere with that. Please try again."  # when it did nothing at all

    def __init__(self, binary: str, workspace: Path, system_prompt_file: Path, model: str, effort: str,
                 sessions: SessionStore, timeout: float, prompt_vars: Mapping[str, str], skills_dir: Path,
                 extensions: list[Path], session_dir: Path,
                 new_session_id: Callable[[], str] = lambda: str(uuid.uuid4())):
        super().__init__(binary, workspace, system_prompt_file, model, effort, sessions, timeout, prompt_vars)
        self.skills_dir = skills_dir
        self.extensions = extensions
        self.session_dir = session_dir
        self.new_session_id = new_session_id

    def _ask(self, text: str, on_event: EventHandler) -> str:
        session_id = self.sessions.load() or self.new_session_id()
        reply = self._run_pi(text, session_id, on_event)
        self.sessions.save(session_id)
        return reply

    def _run_pi(self, text: str, session_id: str, on_event: EventHandler) -> str:
        cmd = [
            self.binary, "--mode", "json", "--print",
            "--model", self.model,
            "--thinking", self.effort,
            "--session-id", session_id,
            "--session-dir", str(self.session_dir),
            "--append-system-prompt", self.system_prompt,
            "--skill", str(self.skills_dir),
            "--no-context-files",  # the repo's CLAUDE.md is for developing saambha, not for the assistant
        ]
        for extension in self.extensions:
            cmd += ["--extension", str(extension)]
        cmd += ["--", text]  # so a message starting with "-" isn't read as an option

        state = {"reply": "", "error": None, "acted": False}

        def handle(event: dict):
            kind = event.get("type")
            if kind == "tool_execution_start":
                state["acted"] = True
                tool = event.get("toolName", "tool")
                path = str((event.get("args") or {}).get("path", ""))
                if tool == "read" and (skill := SKILL_FILE.search(path)):
                    tool = f"skill {skill.group(1)}"
                on_event("tool_use", tool)
            elif kind == "message_end" and event["message"].get("role") == "assistant":
                message = event["message"]
                if message.get("stopReason") == "error":
                    # pi retries on its own; this sticks only if nothing better follows.
                    state["error"] = message.get("errorMessage") or "unknown error"
                    return
                message_text = "".join(
                    block.get("text", "") for block in message.get("content", [])
                    if isinstance(block, dict) and block.get("type") == "text"
                ).strip()
                if message_text:
                    on_event("text", message_text)
                if message.get("stopReason") != "toolUse":  # the last word, not what it says on the way
                    state["reply"] = message_text
                    state["error"] = None

        run = self._run(cmd, handle)
        if run.timed_out:
            return self.TIMEOUT_REPLY
        if run.returncode != 0 and not state["reply"] and not state["error"]:
            # e.g. not logged in: pi says why on stderr ("No API key found for anthropic.")
            state["error"] = next((line for line in run.stderr.splitlines() if line.strip()),
                                  f"pi exited with code {run.returncode}")
        if state["error"] and not state["reply"]:
            log.error(f"pi failed: {state['error']}")
            return f"Sorry, something went wrong: {state['error']}"
        if state["reply"]:
            return state["reply"]
        if not state["acted"]:
            log.warning("pi's model ended without a word or a command")
        return self.DONE_REPLY if state["acted"] else self.SILENT_REPLY
