"""Settings: every tunable value and external endpoint, in one place.

Any of them can be overridden per user with SAAMBHA_<NAME> in server/.env (e.g. SAAMBHA_HARNESS=pi),
which also holds secrets (e.g. the Spotify keys). See .env.sample.
"""
import os
import shutil
import sys
from dataclasses import dataclass, fields, replace
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # server/


def load_env_file(path: Path, environ=os.environ):
    """Copies KEY=value lines from [path] into [environ] without overriding what's already set.

    Into os.environ, so the assistant's own tools (e.g. the Spotify skill) see them.
    """
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, _, value = line.partition("=")
            environ.setdefault(key.strip(), value.strip())


@dataclass(frozen=True)
class Settings:
    port: int
    cert_file: Path
    key_file: Path
    devices_file: Path
    session_file: Path
    session_ttl: int  # seconds without use before the conversation starts over
    history_file: Path  # every command and reply, shown in the chat popup
    # The agent harness that runs commands: "pi" (https://pi.dev) or "claude" (Claude Code).
    harness: str
    # The agent's working folder: system-prompt.md, .claude/ and .pi/ live here.
    workspace: Path
    skills_dir: Path  # the agent's skills (a separate git repo), shared by both harnesses
    agent_timeout: int
    pi_bin: str
    pi_model: str  # provider/model
    pi_session_dir: Path
    claude_bin: str
    claude_model: str
    piper_bin: str
    piper_voice: str
    piper_voices_dir: Path
    piper_voices_url: str
    whisper_model: str
    approval_timeout: int  # seconds a new device waits to be allowed on the desktop before it's denied
    status_wait: int  # seconds the desktop app's status request waits for a change before it's answered
    shutdown_wait: int  # seconds a stopping server gives open requests (like that wait) before cutting them off
    deny_cooldown: int  # seconds before a denied device can ask again
    aplay_bin: str
    # Keeping a local (Ollama) model loaded, except while something heavy runs.
    ollama_url: str
    # scripts/install.sh sets this context window on "ollama/saambha-<model>" (made from "<model>").
    ollama_context: int
    heavy_check_interval: int  # seconds
    heavy_resume_after: int  # seconds of nothing heavy before the model is loaded again
    heavy_gpu_mb: int  # a process using this much GPU memory is heavy
    heavy_min_free_ram_mb: int  # less free RAM than this is heavy
    heavy_cpu_load: float  # a load average above this per core is heavy
    log_level: str
    log_file: Path
    log_max_bytes: int
    log_backups: int
    log_view_bytes: int  # of the log's end the desktop app's log window starts with

    @property
    def system_prompt_file(self) -> Path:
        return self.workspace / "system-prompt.md"

    @property
    def pi_extensions(self) -> list[Path]:
        return [self.workspace / ".pi" / "guard.ts"]

    @classmethod
    def default(cls, root: Path = ROOT, environ=os.environ) -> "Settings":
        """The defaults below, overridden by any SAAMBHA_<NAME> in [environ]."""
        home = Path.home()
        return cls(
            port=8765,
            cert_file=root / "cert.pem",
            key_file=root / "key.pem",
            devices_file=root / "devices.json",
            session_file=home / ".config" / "saambha" / "session_id",
            session_ttl=3600,
            history_file=home / ".local" / "share" / "saambha" / "history.jsonl",
            harness="claude",
            workspace=root,
            skills_dir=home / "saambha-skills",
            agent_timeout=120,
            pi_bin=shutil.which("pi") or str(home / ".local" / "bin" / "pi"),
            # "ollama/..." models run locally and are unloaded during heavy tasks.
            pi_model="anthropic/claude-haiku-5-5",
            pi_session_dir=home / ".local" / "share" / "saambha" / "pi-sessions",
            claude_bin=shutil.which("claude") or str(home / ".local" / "bin" / "claude"),
            claude_model="claude-haiku-5-5",
            # piper is installed in the venv; find it even when the venv isn't activated.
            piper_bin=shutil.which("piper", path=os.path.dirname(sys.executable)) or "piper",
            piper_voice="en_US-lessac-medium",
            piper_voices_dir=home / ".local" / "share" / "piper-voices",
            piper_voices_url="https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0",
            whisper_model="small",
            approval_timeout=60,
            status_wait=25,
            shutdown_wait=3,
            deny_cooldown=300,
            aplay_bin="aplay",
            ollama_url="http://127.0.0.1:11434",
            ollama_context=16384,
            heavy_check_interval=10,
            heavy_resume_after=60,
            heavy_gpu_mb=1500,
            heavy_min_free_ram_mb=1500,
            heavy_cpu_load=0.75,
            log_level="INFO",
            log_file=root / "server.log",
            log_max_bytes=5_000_000,
            log_backups=3,
            log_view_bytes=200_000,
        ).with_overrides(environ)

    def with_overrides(self, environ) -> "Settings":
        """These settings with SAAMBHA_<NAME> values from [environ], parsed as each setting's type."""
        changes = {}
        for field in fields(self):
            raw = environ.get(f"SAAMBHA_{field.name.upper()}")
            if raw is not None:
                changes[field.name] = _parse(raw, getattr(self, field.name))
        return replace(self, **changes)


def _parse(raw: str, like):
    if isinstance(like, Path):
        return Path(raw).expanduser()
    return type(like)(raw)
