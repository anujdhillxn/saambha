"""How well each agent model does Saambha's job: the same commands through every model, --runs times
each (a model that gets it right once in three isn't reliable), each reply checked against the
machine's own answer, timed, and compared in a table at the end (with -v, every reply too).

The commands only look things up, and whatever an agent saves in Saambha's data folder is put back.
Besides single questions, they cover what goes wrong in use: a misheard request, a request after a
long conversation, using a skill twice, and saying which speaker sound is on (which small models
wrongly claim they can't).

Runs the real models, so it costs usage and takes a while: `pytest -m models` compares the configured
claude_model and pi_model at the configured effort (SAAMBHA_EFFORT=low to change it);
`--models claude:claude-haiku-5-5,claude:claude-opus-5-5` picks others (an Ollama model has to be
pulled already), `--runs 1` for a quick look.
"""
import json
import re
import shutil
import subprocess
import sys
from datetime import datetime
from pathlib import Path

import pytest

pytestmark = pytest.mark.models

LUCKY = Path(__file__).parent / "skills" / "lucky-number" / "lucky.py"
# Skills Claude Code has from the user's own setup, which aren't Saambha's.
NOT_SAAMBHAS = re.compile(r"\b(pdf|spreadsheet|slides|powerpoint|excel|word documents?|google (docs|drive|workspace)"
                          r"|code review|deep research)\b", re.I)


def numbers(text: str) -> list[float]:
    return [float(n.replace(",", "")) for n in re.findall(r"\d[\d,]*(?:\.\d+)?", text)]


def minutes_said(text: str) -> list[int]:
    """Every h:mm in [text], as minutes past 12 or 0 o'clock (so 8:40 pm and 20:40 match)."""
    return [int(h) % 12 * 60 + int(m) for h, m in re.findall(r"\b(\d{1,2})[:.](\d{2})\b", text)]


def lucky(name: str) -> int:
    return int(subprocess.run([sys.executable, LUCKY, name], capture_output=True, text=True).stdout)


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


def test_lists_only_its_own_skills(agent):
    reply = agent.ask("Which skills do you have?")

    lower = reply.lower()
    assert "alarm" in lower and ("spotify" in lower or "music" in lower) and "lucky" in lower, reply
    assert not NOT_SAAMBHAS.search(reply), reply


def test_uses_a_skill(agent):
    reply = agent.ask("What is Priya's lucky number?")

    assert lucky("Priya") in numbers(reply), reply


def test_uses_a_skill_twice(agent):
    reply = agent.ask("What do Priya's and Arjun's lucky numbers add up to?")

    assert lucky("Priya") + lucky("Arjun") in numbers(reply), reply


def test_a_misheard_request(agent):
    reply = agent.ask("What's Priya's lucky dumber?")

    assert lucky("Priya") in numbers(reply), reply


def sound_output_words() -> set[str]:
    """Words a reply naming the current sound output would use: from its name, and for HDMI what it is."""
    if not shutil.which("pactl"):
        pytest.skip("no pactl")
    default = subprocess.run(["pactl", "get-default-sink"], capture_output=True, text=True).stdout.strip()
    sinks = json.loads(subprocess.run(["pactl", "-f", "json", "list", "sinks"], capture_output=True, text=True).stdout)
    description = next(s["description"] for s in sinks if s["name"] == default)
    generic = {"output", "stereo", "digital", "audio", "sof", "hda", "dsp", "analog"}
    found = {w for w in re.findall(r"[a-z0-9]+", description.lower()) if len(w) > 2} - generic
    return found | ({"monitor", "display", "screen", "tv"} if "hdmi" in found or "displayport" in found else set())


def test_says_which_speaker_sound_is_on(agent):
    expected = sound_output_words()

    reply = agent.ask("Which speaker is sound playing on right now?")

    assert expected & set(re.findall(r"[a-z0-9]+", reply.lower())), (expected, reply)


def test_arithmetic(agent):
    reply = agent.ask("What is 17 times 23?")

    assert 391 in numbers(reply), reply


def test_follow_up(agent):
    agent.ask("My locker code is 4821.")

    reply = agent.ask("What was my locker code?")

    assert 4821 in numbers(reply), reply


def test_a_skill_after_a_long_conversation(agent):
    agent.chat("What time is it?", "How much free space is on my home disk?", "Which speaker is sound playing on?",
               "What is 12 times 12?", "Thanks, that's all for now.", "Actually, one more: how many CPU cores do I have?")

    reply = agent.ask("What is Arjun's lucky number?")

    assert lucky("Arjun") in numbers(reply), reply
