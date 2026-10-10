from conftest import is_running, wait_until
from saambha.voice.narrator import piper_voice_path

# Narrator (against tests/fakes/piper and aplay)

def test_reply_is_spoken_verbatim_without_running_shell_syntax(narrator, fakes, tmp_path):
    marker = tmp_path / "pwned"
    text = f'It costs $5 `touch {marker}` $(touch {marker}) "quoted" it\'s\nsecond line'

    narrator.speak(text)

    assert wait_until(lambda: fakes.spoken() == text)
    assert not marker.exists()


def test_says_when_it_starts_and_stops_speaking(narrator, fakes):
    narrator.speak("short")  # fake piper finishes straight away

    assert wait_until(lambda: narrator.speaking == [True, False])


def test_new_reply_cuts_off_the_one_playing(narrator, fakes):
    fakes.set("speech_seconds", 30)
    narrator.speak("first")
    assert wait_until(lambda: fakes.spoken() == "first")
    first = fakes.piper_pid()

    narrator.speak("second")

    assert wait_until(lambda: not is_running(first))
    assert wait_until(lambda: fakes.spoken() == "second")
    assert narrator.speaking == [True, False, True]


def test_stop_ends_narration(narrator, fakes):
    fakes.set("speech_seconds", 30)
    narrator.speak("hello")
    assert wait_until(lambda: fakes.spoken() == "hello")

    narrator.stop()

    assert wait_until(lambda: not is_running(fakes.piper_pid()))
    assert narrator.speaking == [True, False]


def test_stop_with_nothing_playing(narrator):
    narrator.stop()

    assert narrator.speaking == []


def test_piper_voice_path(tmp_path):
    assert piper_voice_path(tmp_path, "en_US-lessac-medium") == tmp_path / "en_US-lessac-medium.onnx"
