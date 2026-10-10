"""Composition root: the only place that wires the real implementations together."""
import logging

from fastapi import FastAPI

from saambha.agent.base import Harness
from saambha.agent.claude_code import ClaudeCodeHarness
from saambha.agent.pi import PiHarness
from saambha.agent.sessions import SessionStore
from saambha.history import ChatHistory
from saambha.logs import LogFile, configure_logging
from saambha.model.activity import GpuMemoryHog, HeavyTaskDetector, HighCpuLoad, LowMemory
from saambha.model.ollama import OllamaClient
from saambha.model.warden import ModelWarden
from saambha.server.app import Services, create_app
from saambha.server.auth import DeviceRegistry, Pairing
from saambha.server.status import Status
from saambha.settings import ROOT, Settings, load_env_file
from saambha.voice.narrator import Narrator, ensure_piper_voice, piper_voice_path
from saambha.voice.stt import WhisperTranscriber

log = logging.getLogger(__name__)


def build_harness(settings: Settings) -> Harness:
    """The agent harness named by settings.harness."""
    sessions = SessionStore(settings.session_file, settings.session_ttl)
    # $names the system prompt can use
    prompt_vars = {"workspace": str(settings.workspace), "skills_dir": str(settings.skills_dir)}
    if settings.harness == "pi":
        return PiHarness(
            binary=settings.pi_bin,
            workspace=settings.workspace,
            system_prompt_file=settings.system_prompt_file,
            model=settings.pi_model,
            sessions=sessions,
            timeout=settings.agent_timeout,
            prompt_vars=prompt_vars,
            skills_dir=settings.skills_dir,
            extensions=settings.pi_extensions,
            session_dir=settings.pi_session_dir,
        )
    if settings.harness == "claude":
        return ClaudeCodeHarness(
            binary=settings.claude_bin,
            workspace=settings.workspace,
            system_prompt_file=settings.system_prompt_file,
            model=settings.claude_model,
            sessions=sessions,
            timeout=settings.agent_timeout,
            prompt_vars=prompt_vars,
            skills_dir=settings.skills_dir,
        )
    raise ValueError(f'Unknown harness {settings.harness!r}: use "pi" or "claude"')


def build_model_warden(settings: Settings, harness: Harness, status: Status) -> ModelWarden | None:
    """For a local Ollama model (pi_model "ollama/..."): keeps it loaded except while something heavy runs."""
    provider, _, model = settings.pi_model.partition("/")
    if settings.harness != "pi" or provider != "ollama":
        return None
    return ModelWarden(
        ollama=OllamaClient(settings.ollama_url),
        model=model,
        heavy_task=HeavyTaskDetector([
            GpuMemoryHog(settings.heavy_gpu_mb),
            LowMemory(settings.heavy_min_free_ram_mb),
            HighCpuLoad(settings.heavy_cpu_load),
        ]).heavy_task,
        busy=lambda: harness.busy,
        on_cold=status.set_model_cold,
        interval=settings.heavy_check_interval,
        resume_after=settings.heavy_resume_after,
    )


def build_services(settings: Settings) -> Services:
    devices = DeviceRegistry(settings.devices_file)
    harness = build_harness(settings)
    status = Status(settings.approval_timeout, settings.status_wait)
    warden = build_model_warden(settings, harness, status)

    def on_startup():
        ensure_piper_voice(settings.piper_voices_dir, settings.piper_voice, settings.piper_voices_url)
        if warden:
            warden.start()

    def on_shutdown():
        if warden:
            warden.stop()

    return Services(
        assistant=harness,
        devices=devices,
        pairing=Pairing(devices, status.ask, deny_cooldown=settings.deny_cooldown),
        speaker=Narrator(
            piper=settings.piper_bin,
            voice_path=piper_voice_path(settings.piper_voices_dir, settings.piper_voice),
            on_speaking=status.set_speaking,
            aplay=settings.aplay_bin,
        ),
        status=status,
        history=ChatHistory(settings.history_file),
        log=LogFile(settings.log_file, settings.log_view_bytes),
        transcriber=WhisperTranscriber(settings.whisper_model),
        on_startup=on_startup,
        on_shutdown=on_shutdown,
    )


def load_settings() -> Settings:
    load_env_file(ROOT / ".env")  # secrets only
    return Settings.default()


def build_app() -> FastAPI:
    """App factory for uvicorn (`--factory`), so auto-reload rebuilds everything."""
    settings = load_settings()
    configure_logging(settings)
    log.info(f"Starting on https://[::]:{settings.port} with the {settings.harness} harness")
    return create_app(build_services(settings))
