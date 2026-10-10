"""The Claude Code harness: `claude -p` in the workspace.

Claude Code finds the sudo / rm -rf guard in .claude/settings.json because it runs with the
workspace as its cwd. It only looks for skills in .claude/skills, so that is kept a link to skills_dir.
"""
import logging
import os
from collections.abc import Mapping
from pathlib import Path

from saambha.agent.base import EventHandler, Harness
from saambha.agent.sessions import SessionStore

log = logging.getLogger(__name__)


class ClaudeCodeHarness(Harness):
    name = "claude"

    def __init__(self, binary: str, workspace: Path, system_prompt_file: Path, model: str,
                 sessions: SessionStore, timeout: float, prompt_vars: Mapping[str, str], skills_dir: Path):
        super().__init__(binary, workspace, system_prompt_file, model, sessions, timeout, prompt_vars)
        self.skills_dir = skills_dir
        self._link_skills()

    def _link_skills(self):
        link = self.workspace / ".claude" / "skills"
        target = os.path.relpath(self.skills_dir, link.parent)
        if link.is_symlink() and os.readlink(link) == target:
            return
        if link.exists() and not link.is_symlink():
            raise RuntimeError(f"{link} is a folder; move its skills to {self.skills_dir}")
        link.parent.mkdir(parents=True, exist_ok=True)
        link.unlink(missing_ok=True)
        link.symlink_to(target)
        log.info(f"Linked {link} -> {target}")

    def _ask(self, text: str, on_event: EventHandler) -> str:
        session_id = self.sessions.load()
        reply, ok = self._run_claude(text, session_id, on_event)
        if not ok and session_id:
            # e.g. the saved session no longer exists — start fresh rather than fail.
            log.warning("Resume failed, retrying with a new session")
            self.sessions.clear()
            reply, ok = self._run_claude(text, None, on_event)
        return reply

    def _run_claude(self, text: str, session_id: str | None, on_event: EventHandler) -> tuple[str, bool]:
        cmd = [
            self.binary, "-p", text,
            "--output-format", "stream-json",
            "--model", self.model,
            "--allowedTools", "Bash", "Skill",
            "--system-prompt", self.system_prompt,
            "--verbose",
        ]
        if session_id:
            log.info(f"Resuming session {session_id}")
            cmd.extend(["--resume", session_id])

        state = {"reply": "", "got_result": False}

        def handle(event: dict):
            if event.get("type") == "assistant" and "message" in event:
                for block in event["message"].get("content", []):
                    if not isinstance(block, dict):
                        continue
                    if block.get("type") == "text":
                        block_text = block["text"].strip()
                        if block_text:
                            on_event("text", block_text)
                        state["reply"] = block_text
                    elif block.get("type") == "tool_use":
                        tool = block.get("name", "tool")
                        if tool == "Skill":
                            tool = f"skill {block.get('input', {}).get('skill', '')}".strip()
                        on_event("tool_use", tool)
            elif event.get("type") == "result":
                state["got_result"] = True
                if result := event.get("result", "").strip():
                    state["reply"] = result
                if sid := event.get("session_id"):
                    self.sessions.save(sid)

        run = self._run(cmd, handle)
        if run.timed_out:
            return self.TIMEOUT_REPLY, True
        return state["reply"], state["got_result"]
