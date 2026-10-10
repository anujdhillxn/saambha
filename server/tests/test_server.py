import asyncio
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from saambha.history import ChatHistory
from saambha.logs import LogFile
from saambha.server.app import Services, create_app
from saambha.server.auth import Pairing
from saambha.server.status import Status

LAN_DEVICE = ("192.168.0.12", 50000)


class FakeAssistant:
    def __init__(self):
        self.asked = []
        self.session = False

    def ask(self, text, on_event=None):
        self.asked.append(text)
        self.session = True
        if on_event:
            on_event("tool_use", "skill spotify")
            on_event("text", f"You said: {text}")
        return f"You said: {text}"

    def reset(self):
        self.session = False

    def has_session(self):
        return self.session


class FakeSpeaker:
    def __init__(self):
        self.spoken = []
        self.stops = 0

    def speak(self, text):
        self.spoken.append(text)

    def stop(self):
        self.stops += 1


class RecordedStatus(Status):
    """The real status, also recording each task it was told about, and how many exchanges the
    history had by then."""

    def __init__(self, history=None):
        super().__init__(approval_timeout=5, change_wait=5)
        self.history = history
        self.shown = []
        self.exchanges = []

    def set_task(self, task):
        super().set_task(task)
        self.shown.append(task)
        self.exchanges.append(len(self.history.recent(100)))


class FakeTranscriber:
    def __init__(self, result="what time is it"):
        self.result = result
        self.uploads = []

    def transcribe(self, audio_path):
        self.uploads.append(Path(audio_path).read_bytes())
        if isinstance(self.result, Exception):
            raise self.result
        return self.result


class FakeDialog:
    def __init__(self, answer=True):
        self.answer = answer
        self.asked = []

    async def __call__(self, ip, name):
        self.asked.append((ip, name))
        return self.answer


@pytest.fixture
def services(registry, clock, tmp_path) -> Services:
    history = ChatHistory(tmp_path / "history.jsonl", clock)
    return Services(
        assistant=FakeAssistant(),
        devices=registry,
        pairing=Pairing(registry, FakeDialog(), deny_cooldown=300, clock=clock),
        speaker=FakeSpeaker(),
        status=RecordedStatus(history),
        history=history,
        log=LogFile(tmp_path / "server.log", backlog=1000),
        transcriber=FakeTranscriber(),
        hostname="desktop",
    )


@pytest.fixture
def device(services):
    """A client on the LAN that isn't paired."""
    return TestClient(create_app(services), client=LAN_DEVICE)


@pytest.fixture
def paired(services):
    """A client on the LAN with a valid token."""
    token = services.devices.add("Phone", LAN_DEVICE[0])
    return TestClient(create_app(services), client=LAN_DEVICE, headers={"Authorization": f"Bearer {token}"})


def test_startup_and_shutdown_hooks_run(services):
    calls = []
    services.on_startup = lambda: calls.append("startup")
    services.on_shutdown = lambda: calls.append("shutdown")

    with TestClient(create_app(services), client=LAN_DEVICE):
        assert calls == ["startup"]

    assert calls == ["startup", "shutdown"]


def test_discover_identifies_the_server_without_a_token(device):
    res = device.get("/discover")

    assert res.status_code == 200
    assert res.json() == {"service": "saambha", "name": "desktop"}


@pytest.mark.parametrize("method, path", [
    ("GET", "/status"), ("POST", "/command"), ("POST", "/voice"),
    ("POST", "/reset"), ("POST", "/listen"), ("DELETE", "/listen"), ("GET", "/docs"),
])
def test_unpaired_device_is_turned_away(device, method, path):
    assert device.request(method, path).status_code == 401


@pytest.mark.parametrize("method, path", [
    ("GET", "/desktop/status"), ("POST", "/desktop/pairing"), ("POST", "/desktop/speech/stop"),
    ("GET", "/desktop/history"), ("GET", "/desktop/log"),
])
def test_desktop_app_endpoints_are_only_for_this_machine(paired, method, path):
    assert paired.request(method, path).status_code == 403


def test_bad_token_is_turned_away(device):
    res = device.get("/status", headers={"Authorization": "Bearer nope"})

    assert res.status_code == 401


def test_this_machine_needs_no_token(services):
    local = TestClient(create_app(services), client=("127.0.0.1", 50000))

    assert local.get("/status").status_code == 200


# /pair

def test_pair_returns_a_working_token_when_allowed(device, services):
    res = device.post("/pair", json={"name": " Pixel Watch 4 "})

    assert res.status_code == 200
    assert res.json()["name"] == "desktop"
    assert services.devices.is_valid(res.json()["token"])
    assert services.pairing.ask.asked == [("192.168.0.12", "Pixel Watch 4")]


def test_paired_device_can_use_its_token(device):
    token = device.post("/pair", json={"name": "Phone"}).json()["token"]

    assert device.get("/status", headers={"Authorization": f"Bearer {token}"}).status_code == 200


def test_pair_without_a_name_says_unknown_device(device, services):
    device.post("/pair", json={})

    assert services.pairing.ask.asked == [("192.168.0.12", "Unknown device")]


def test_pair_denied(device, services):
    services.pairing.ask.answer = False

    assert device.post("/pair", json={"name": "Phone"}).status_code == 403


def test_pair_while_another_device_is_waiting(device, services):
    class Busy:
        async def request(self, ip, name):
            raise RuntimeError("Another device is awaiting approval")
    services.pairing = Busy()

    res = device.post("/pair", json={"name": "Phone"})

    assert res.status_code == 429
    assert res.json()["detail"] == "Another device is awaiting approval"


# /command

def test_command_replies_and_speaks_on_the_desktop(paired, services):
    res = paired.post("/command", json={"text": "  play music  "})

    assert res.status_code == 200
    assert res.json() == {"input": "play music", "text": "You said: play music"}
    assert services.assistant.asked == ["play music"]
    assert services.speaker.spoken == ["You said: play music"]
    assert services.speaker.stops == 1  # cuts off the previous reply first
    assert [(e.said, e.reply) for e in services.history.recent(10)] == [("play music", "You said: play music")]


def test_command_for_the_phone_is_not_spoken_on_the_desktop(paired, services):
    res = paired.post("/command", json={"text": "play music", "speak": "none"})

    assert res.json()["text"] == "You said: play music"
    assert services.speaker.spoken == []


def test_desktop_shows_what_the_agent_is_doing(paired, services):
    paired.post("/command", json={"text": "play music"})

    assert services.status.shown == ["Thinking", "Running skill spotify", None]


def test_command_is_in_the_history_by_the_time_it_stops_showing(paired, services):
    paired.post("/command", json={"text": "play music"})

    assert services.status.exchanges[-1] == 1  # so the panel, told of the change, shows it


def test_desktop_stops_showing_a_command_that_failed(paired, services):
    def fail(text, on_event=None):
        raise RuntimeError("agent crashed")
    services.assistant.ask = fail

    with pytest.raises(RuntimeError):
        paired.post("/command", json={"text": "play music"})

    assert services.status.shown == ["Thinking", None]


def test_empty_command_is_rejected(paired, services):
    res = paired.post("/command", json={"text": "   "})

    assert res.status_code == 400
    assert services.assistant.asked == []


@pytest.mark.parametrize("kwargs", [{"json": {}}, {"content": "play music"}])
def test_malformed_command_is_rejected(paired, services, kwargs):
    assert paired.post("/command", **kwargs).status_code == 422
    assert services.assistant.asked == []


# /voice

def test_voice_is_transcribed_and_run(paired, services):
    services.status.set_listening(True)
    res = paired.post("/voice", files={"audio": ("rec.wav", b"RIFF-audio", "audio/wav")})

    assert res.json()["text"] == "You said: what time is it"
    assert services.transcriber.uploads == [b"RIFF-audio"]
    assert services.speaker.spoken == ["You said: what time is it"]
    assert services.speaker.stops == 1
    assert services.status.activity.listening is False


def test_unreadable_recording(paired, services):
    services.transcriber.result = ValueError("bad audio")

    res = paired.post("/voice", files={"audio": ("rec.wav", b"", "audio/wav")})

    assert res.status_code == 400
    assert res.json()["detail"] == "Could not read the recording"


def test_silent_recording(paired, services):
    services.transcriber.result = ""

    res = paired.post("/voice", files={"audio": ("rec.wav", b"RIFF", "audio/wav")})

    assert res.status_code == 400
    assert services.assistant.asked == []


# Other endpoints

def test_listening_cuts_off_the_narration(paired, services):
    assert paired.post("/listen").status_code == 200

    assert services.speaker.stops == 1
    assert services.status.activity.listening is True


def test_listening_ends_without_a_recording(paired, services):
    paired.post("/listen")

    assert paired.delete("/listen").status_code == 200
    assert services.status.activity.listening is False


# The desktop app's endpoints

@pytest.fixture
def desktop(services):
    """The desktop app: a client on this machine."""
    return TestClient(create_app(services), client=("127.0.0.1", 50000))


def test_desktop_status(desktop, services):
    services.status.set_model_cold("Loading the model")

    res = desktop.get("/desktop/status")

    assert res.json()["state"] == "cold"
    assert res.json()["line"] == "Loading the model"


def test_desktop_status_after_a_version_it_has_seen(desktop, services):
    services.status.set_listening(True)

    res = desktop.get("/desktop/status", params={"after": 0})

    assert (res.json()["state"], res.json()["version"]) == ("listening", 1)


def test_desktop_allows_a_waiting_device(device, desktop, services):
    answered = []

    async def ask_and_answer(ip, name):
        asking = asyncio.create_task(services.status.ask(ip, name))
        await asyncio.sleep(0)
        answered.append(desktop.post("/desktop/pairing", json={"allow": True}).status_code)
        return await asking
    services.pairing.ask = ask_and_answer

    res = device.post("/pair", json={"name": "Phone"})

    assert answered == [200]
    assert res.status_code == 200


def test_desktop_answer_with_no_device_waiting(desktop):
    assert desktop.post("/desktop/pairing", json={"allow": True}).status_code == 409


def test_desktop_stops_the_speech(desktop, services):
    assert desktop.post("/desktop/speech/stop").status_code == 200
    assert services.speaker.stops == 1


def test_status_and_reset(paired, services):
    paired.post("/command", json={"text": "hi", "speak": "none"})
    assert paired.get("/status").json()["has_session"] is True

    paired.post("/reset")

    assert paired.get("/status").json()["has_session"] is False


def test_reset_starts_a_new_conversation_in_the_history(desktop, services, clock):
    desktop.post("/command", json={"text": "hi", "speak": "none"})
    clock.advance(5)

    desktop.post("/reset")

    assert desktop.get("/desktop/history").json()[-1] == {"time": clock.now, "new_conversation": True}


def test_desktop_history(desktop, services, clock):
    services.history.add("play music", "Playing")
    clock.advance(5)
    services.history.add("louder", "Done")

    res = desktop.get("/desktop/history", params={"limit": 1})

    assert res.json() == [{"time": clock.now, "said": "louder", "reply": "Done"}]


def test_desktop_log_follows_the_log(desktop, services):
    services.log.path.write_text("line 1\n")

    first = desktop.get("/desktop/log").json()
    with services.log.path.open("a") as f:
        f.write("line 2\n")
    after = desktop.get("/desktop/log", params={"offset": first["offset"]}).json()

    assert first == {"text": "line 1\n", "offset": 7}
    assert after == {"text": "line 2\n", "offset": 14}
