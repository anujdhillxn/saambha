from pathlib import Path

import pytest

from saambha.settings import Settings, load_env_file


def test_paths_are_under_the_server_root(tmp_path):
    settings = Settings.default(root=tmp_path, environ={})

    assert settings.devices_file == tmp_path / "devices.json"
    assert settings.cert_file == tmp_path / "cert.pem"
    assert settings.workspace == tmp_path
    assert settings.system_prompt_file == tmp_path / "system-prompt.md"
    assert settings.pi_extensions == [tmp_path / ".pi" / "guard.ts"]
    assert settings.session_file == Path.home() / ".config" / "saambha" / "session_id"
    assert settings.history_file == Path.home() / ".local" / "share" / "saambha" / "history.jsonl"


def test_per_user_defaults():
    settings = Settings.default(environ={})

    assert settings.harness == "claude"
    assert settings.pi_model == "anthropic/claude-haiku-5-5"
    assert settings.skills_dir == Path.home() / "saambha-skills"


def test_any_setting_can_be_overridden_as_its_type():
    settings = Settings.default(environ={
        "SAAMBHA_HARNESS": "pi",
        "SAAMBHA_PI_MODEL": "ollama/gemma4:e4b",
        "SAAMBHA_SKILLS_DIR": "~/my-skills",
        "SAAMBHA_HEAVY_GPU_MB": "2000",
        "SAAMBHA_HEAVY_CPU_LOAD": "0.9",
    })

    assert settings.harness == "pi"
    assert settings.pi_model == "ollama/gemma4:e4b"
    assert settings.skills_dir == Path.home() / "my-skills"
    assert settings.heavy_gpu_mb == 2000
    assert settings.heavy_cpu_load == 0.9


def test_unrelated_variables_are_ignored():
    assert Settings.default(environ={"SAAMBHA_NOT_A_SETTING": "x", "HARNESS": "pi"}) == Settings.default(environ={})


def test_bad_value_fails_loudly():
    with pytest.raises(ValueError):
        Settings.default(environ={"SAAMBHA_HEAVY_GPU_MB": "lots"})


def test_env_file_fills_in_without_overriding(tmp_path):
    env_file = tmp_path / ".env"
    env_file.write_text("# comment\nSPOTIFY_CLIENT_ID = abc \nSPOTIFY_CLIENT_SECRET=from-file\n\nnot a setting\n")
    environ = {"SPOTIFY_CLIENT_SECRET": "already-set"}

    load_env_file(env_file, environ)

    assert environ == {"SPOTIFY_CLIENT_SECRET": "already-set", "SPOTIFY_CLIENT_ID": "abc"}


def test_missing_env_file(tmp_path):
    environ = {}

    load_env_file(tmp_path / ".env", environ)

    assert environ == {}
