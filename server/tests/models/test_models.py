"""How well each agent model does Saambha's job: the same commands through every model, each
checked against the machine's own answer, timed, and compared in a table at the end (with -v, every
reply too). The commands only look things up, so running them changes nothing.

Runs the real models, so it costs usage and takes minutes: `pytest -m models` compares the
configured claude_model and pi_model; `--models claude:claude-haiku-5-5,pi:ollama/saambha-qwen3.5:4b`
picks others (an Ollama model has to be pulled already).
"""
import re
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

import pytest

pytestmark = pytest.mark.models

LUCKY = Path(__file__).parent / "skills" / "lucky-number" / "lucky.py"


def numbers(text: str) -> list[float]:
    return [float(n.replace(",", "")) for n in re.findall(r"\d[\d,]*(?:\.\d+)?", text)]


def minutes_said(text: str) -> list[int]:
    """Every h:mm in [text], as minutes past 12 or 0 o'clock (so 8:40 pm and 20:40 match)."""
    return [int(h) % 12 * 60 + int(m) for h, m in re.findall(r"\b(\d{1,2})[:.](\d{2})\b", text)]


def test_time(agent):
    now = datetime.now()

    reply = agent.ask("What time is it?")

    expected = now.hour % 12 * 60 + now.minute
    assert any(min(abs(said - expected), 720 - abs(said - expected)) <= 2 for said in minutes_said(reply)), reply


def test_weekday(agent):
    reply = agent.ask("What day of the week is it today?").lower()

    days = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
    assert [day for day in days if day in reply] == [datetime.now().strftime("%A").lower()], reply


def test_free_disk_space(agent):
    reply = agent.ask("How much free space is on my home disk, in gigabytes?")

    free = shutil.disk_usage(Path.home()).free
    assert any(abs(said - size) <= size * 0.1 for said in numbers(reply) for size in (free / 1e9, free / 2**30)), reply


def test_lines_in_a_file(agent, tmp_path):
    notes = tmp_path / "notes.txt"
    notes.write_text("".join(f"note {n}\n" for n in range(137)))

    reply = agent.ask(f"How many lines are in {notes}?")

    assert 137 in numbers(reply), reply


def test_lists_its_skills(agent):
    reply = agent.ask("Which skills do you have?").lower()

    assert "alarm" in reply and ("spotify" in reply or "music" in reply) and "lucky" in reply, reply


def test_uses_a_skill(agent):
    expected = int(subprocess.run([sys.executable, LUCKY, "Priya"], capture_output=True, text=True).stdout)

    reply = agent.ask("What is Priya's lucky number?")

    assert expected in numbers(reply), reply


def test_arithmetic(agent):
    reply = agent.ask("What is 17 times 23?")

    assert 391 in numbers(reply), reply


def test_follow_up(agent):
    agent.ask("My locker code is 4821.")

    reply = agent.ask("What was my locker code?")

    assert 4821 in numbers(reply), reply
