import os

from saambha.logs import LogFile


def test_starts_with_the_end_of_the_log_from_a_line_start(tmp_path):
    log = tmp_path / "server.log"
    log.write_text("".join(f"line {n}\n" for n in range(100)))

    assert LogFile(log, backlog=20).read(None) == ("line 98\nline 99\n", log.stat().st_size)


def test_short_log_is_read_whole(tmp_path):
    log = tmp_path / "server.log"
    log.write_text("line 1\nline 2\n")

    assert LogFile(log, backlog=1000).read(None) == ("line 1\nline 2\n", 14)


def test_carries_on_from_the_offset(tmp_path):
    log = tmp_path / "server.log"
    log.write_text("line 1\n")
    logs = LogFile(log, backlog=1000)
    _, offset = logs.read(None)

    assert logs.read(offset) == ("", offset)
    with log.open("a") as f:
        f.write("line 2\nhalf a ")
    text, offset = logs.read(offset)
    assert text == "line 2\nhalf a "
    with log.open("a") as f:
        f.write("line\n")
    assert logs.read(offset)[0] == "line\n"


def test_starts_over_after_rotation(tmp_path):
    log = tmp_path / "server.log"
    log.write_text("old 1\nold 2\n")
    logs = LogFile(log, backlog=1000)
    _, offset = logs.read(None)

    os.rename(log, tmp_path / "server.log.1")
    log.write_text("new\n")

    assert logs.read(offset) == ("new\n", 4)


def test_no_log_yet(tmp_path):
    assert LogFile(tmp_path / "server.log", backlog=1000).read(None) == ("", 0)
