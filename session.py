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


def ask(user_text: str, on_event=None) -> str:
    """Run Claude CLI with stream-json and call on_event(event_type, detail) in real-time.

    event_type is one of: "text", "tool_use", "result".
    """
    if _is_expired():
        log.info(f"Session expired after {SESSION_TTL}s, starting new")
        reset()

    session_id = _load_session_id()
    model = os.environ.get("CLAUDE_MODEL", "claude-haiku-4-5-20251001")
    cmd = [
        CLAUDE, "-p", user_text,
        "--output-format", "stream-json",
        "--model", model,
        "--allowedTools", "Bash",
        "--system-prompt", _load_system_prompt(),
        "--verbose"
    ]
    if session_id:
        log.info(f"Resuming session {session_id}")
        cmd.extend(["--resume", session_id])

    proc = subprocess.Popen(
        cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
    )

    final_text = ""
    try:
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                data = json.loads(line)
            except json.JSONDecodeError:
                continue

            msg_type = data.get("type")
            log.debug(f"STREAM raw: {json.dumps(data)[:500]}")

            if msg_type == "assistant" and "message" in data:
                for block in data["message"].get("content", []):
                    if isinstance(block, dict):
                        if block.get("type") == "text":
                            text = block["text"].strip()
                            if text and on_event:
                                on_event("text", text)
                            final_text = text
                        elif block.get("type") == "tool_use":
                            tool_name = block.get("name", "tool")
                            if on_event:
                                on_event("tool_use", tool_name)

            elif msg_type == "result":
                result_text = data.get("result", "").strip()
                if result_text:
                    final_text = result_text
                sid = data.get("session_id")
                if sid:
                    try:
                        _save_session_id(sid)
                    except Exception as e:
                        log.warning(f"Failed to save session: {e}")
                if on_event:
                    on_event("result", final_text)

        proc.wait(timeout=120)
    except subprocess.TimeoutExpired:
        proc.kill()
        log.error("Claude timed out")
        return "Sorry, that took too long. Please try again."

    if proc.returncode != 0:
        stderr = proc.stderr.read().strip()
        log.error(f"Claude exited {proc.returncode}: {stderr}")

    return final_text


def reset():
    if os.path.exists(SESSION_FILE):
        os.unlink(SESSION_FILE)
