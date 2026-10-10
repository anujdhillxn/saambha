import dataclasses
import logging

import pytest

from saambha.logs import UVICORN_LOGGERS, configure_logging
from saambha.settings import Settings


@pytest.fixture
def settings(tmp_path):
    return dataclasses.replace(Settings.default(root=tmp_path), log_max_bytes=2_000, log_backups=2)


@pytest.fixture(autouse=True)
def restore_logging():
    """configure_logging replaces the root logger's handlers, including pytest's own."""
    root = logging.getLogger()
    saved = root.handlers[:], root.level
    saved_uvicorn = {name: (logging.getLogger(name).handlers[:], logging.getLogger(name).propagate)
                     for name in UVICORN_LOGGERS}
    yield
    for handler in root.handlers[:]:
        root.removeHandler(handler)
        handler.close()
    root.handlers[:], root.level = saved
    for name, (handlers, propagate) in saved_uvicorn.items():
        logging.getLogger(name).handlers[:] = handlers
        logging.getLogger(name).propagate = propagate


def lines(settings):
    return settings.log_file.read_text().splitlines()


def test_module_logs_go_to_the_file_with_level_and_module(settings):
    configure_logging(settings)

    logging.getLogger("saambha.server").info("INPUT: hello")

    [line] = lines(settings)
    assert line.endswith(" INFO saambha.server: INPUT: hello")


def test_level_comes_from_settings(settings):
    configure_logging(dataclasses.replace(settings, log_level="WARNING"))

    logging.getLogger("saambha.server.auth").info("quiet")
    logging.getLogger("saambha.server.auth").warning("loud")

    assert [line.split(": ", 1)[1] for line in lines(settings)] == ["loud"]


def test_uvicorn_logs_use_the_same_format(settings):
    logging.getLogger("uvicorn.access").addHandler(logging.StreamHandler())  # as uvicorn's own config would
    configure_logging(settings)

    logging.getLogger("uvicorn.access").info('"GET /status HTTP/1.1" 200')

    assert lines(settings) == [lines(settings)[0]]
    assert lines(settings)[0].endswith(' INFO uvicorn.access: "GET /status HTTP/1.1" 200')
    assert logging.getLogger("uvicorn.access").handlers == []


def test_configuring_again_does_not_duplicate_lines(settings):
    configure_logging(settings)
    configure_logging(settings)

    logging.getLogger("saambha").info("once")

    assert len(lines(settings)) == 1


def test_log_file_is_rotated(settings):
    configure_logging(settings)

    for i in range(200):
        logging.getLogger("saambha").info(f"line {i} " + "x" * 40)

    files = sorted(p.name for p in settings.log_file.parent.glob("server.log*"))
    assert files == ["server.log", "server.log.1", "server.log.2"]
    assert settings.log_file.stat().st_size <= 2_000


def test_console_only(settings, capsys):
    configure_logging(settings, to_file=False)

    logging.getLogger("saambha").info("hello")

    assert not settings.log_file.exists()
    assert "INFO saambha: hello" in capsys.readouterr().err
