import shutil
import subprocess
import json
import logging
import os
import time

log = logging.getLogger("voice-ai")

CLAUDE = shutil.which("claude") or os.path.expanduser("~/.local/bin/claude")
SESSION_FILE = os.path.expanduser("~/.config/voice-ai/session_id")
SESSION_TTL = int(os.environ.get("SESSION_TTL_SECONDS", 3600))

PROMPTS_DIR = os.path.join(os.path.dirname(__file__), "prompts")


def _load_system_prompt():
    parts = []
    base = os.path.join(PROMPTS_DIR, "base.txt")
    if os.path.exists(base):
        with open(base) as f:
            parts.append(f.read().strip())
    try:
        filenames = sorted(os.listdir(PROMPTS_DIR))
    except OSError as e:
        log.warning(f"Could not read prompts directory: {e}")
        return "\n\n".join(parts)
    for name in filenames:
        if name == "base.txt" or not name.endswith(".txt"):
            continue
        try:
            with open(os.path.join(PROMPTS_DIR, name)) as f:
                parts.append(f.read().strip())
        except OSError as e:
            log.warning(f"Skipping prompt {name}: {e}")
    return "\n\n".join(parts)

os.makedirs(os.path.dirname(SESSION_FILE), exist_ok=True)


def _load_session_id():
    if os.path.exists(SESSION_FILE):
        with open(SESSION_FILE) as f:
            return f.read().strip() or None
    return None


def _save_session_id(session_id):
    with open(SESSION_FILE, "w") as f:
        f.write(session_id)
    log.info(f"Session saved {session_id}")


def _is_expired():
    if not os.path.exists(SESSION_FILE):
        return False
    return time.time() - os.path.getmtime(SESSION_FILE) > SESSION_TTL


def ask(user_text: str) -> str:
    if _is_expired():
        log.info(f"Session expired after {SESSION_TTL}s, starting new")
        reset()

    session_id = _load_session_id()
    model = os.environ.get("CLAUDE_MODEL", "claude-haiku-4-5-20251001")
    cmd = [
        CLAUDE, "-p", user_text,
        "--output-format", "json",
        "--model", model,
        "--allowedTools", "Bash",
        "--system-prompt", _load_system_prompt(),
    ]
    if session_id:
        log.info(f"Resuming session {session_id}")
        cmd.extend(["--resume", session_id])

    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True,
            timeout=120
        )
    except subprocess.TimeoutExpired:
        log.error("Claude timed out after 120s")
        return "Sorry, that took too long. Please try again."

    try:
        data = json.loads(result.stdout.strip())
        if "session_id" in data:
            try:
                _save_session_id(data["session_id"])
            except Exception as e:
                log.warning(f"Failed to save session: {e}")
        return data.get("result", "").strip()
    except (json.JSONDecodeError, KeyError):
        if result.returncode != 0:
            log.error(f"Claude exited {result.returncode}: {result.stderr.strip()}")
        return result.stdout.strip()


def reset():
    if os.path.exists(SESSION_FILE):
        os.unlink(SESSION_FILE)
