"""End to end: the real server over HTTPS, driven the way the phone/watch apps and the hotkey use it.

The server runs as its own process (serve.py: the real wiring) with a fresh CA from
../scripts/gen_certs.sh and the fakes in tests/fakes standing in for the agent and piper. Requests from
127.0.0.2 / 127.0.0.3 stand in for LAN devices; only 127.0.0.1 counts as this machine, where the
desktop app runs.
"""
import asyncio
import contextlib
import json
import os
import shutil
import socket
import ssl
import subprocess
import sys
import threading
import time
from pathlib import Path

import httpx
import pytest
from websockets.asyncio.client import connect
from websockets.exceptions import InvalidStatus

from conftest import Fakes, is_running, wait_until
from saambha.settings import Settings
from usage import measure

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="module", params=["pi", "claude"])
def server(request, tmp_path_factory):
    """The server, once per harness."""
    with running_server(tmp_path_factory.mktemp(f"e2e-{request.param}"), request.param) as server:
        yield server


@contextlib.contextmanager
def running_server(work: Path, harness: str):
    subprocess.run([ROOT.parent / "scripts" / "gen_certs.sh", work], check=True, capture_output=True)
    (work / "test-voice.onnx").touch()
    shutil.copy(ROOT / "system-prompt.md", work)
    state = work / "fakes"
    state.mkdir()

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    env = {**os.environ, "PYTHONPATH": str(ROOT / "src"),
           "SAAMBHA_E2E_DIR": str(work), "SAAMBHA_FAKES_STATE": str(state)}
    with open(work / "server.log", "w") as log:  # the server keeps its own handle to it
        proc = subprocess.Popen([sys.executable, Path(__file__).parent / "serve.py", str(port), harness],
                                cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)

    url = f"https://127.0.0.1:{port}"
    ca = ssl.create_default_context(cafile=work / "ca.pem")

    def up():
        try:
            return httpx.get(f"{url}/discover", verify=ca).status_code == 200
        except httpx.HTTPError:
            return False
    try:
        assert wait_until(up, timeout=20), (work / "server.log").read_text()
        yield type("Server", (), {"url": url, "ca": ca, "work": work, "fakes": Fakes(state),
                                  "harness": harness, "proc": proc, "pid": proc.pid})
    finally:
        proc.terminate()
        proc.wait(timeout=10)


@pytest.fixture
def fakes(server):
    """Clears what the fakes recorded in earlier tests (the server keeps running)."""
    for f in server.fakes.state.iterdir():
        f.unlink()
    return server.fakes


def device(server, ip="127.0.0.2", token=None) -> httpx.Client:
    """A client connecting from [ip], trusting only the server's CA, like the apps."""
    return httpx.Client(
        base_url=server.url,
        transport=httpx.HTTPTransport(local_address=ip, verify=server.ca),
        headers={"Authorization": f"Bearer {token}"} if token else {},
        timeout=30,
    )


def desktop_status(server) -> dict:
    """What the desktop app sees."""
    with device(server, ip="127.0.0.1") as d:
        return d.get("/desktop/status").json()


def ask_to_pair(server, ip: str, name: str, allow: bool) -> tuple[httpx.Response, str]:
    """POST /pair from [ip], answered on the desktop as the popup does; also what the desktop
    app was shown while it was asked."""
    result = {}

    def request():
        with device(server, ip) as d:
            result["response"] = d.post("/pair", json={"name": name})
    thread = threading.Thread(target=request)
    thread.start()
    assert wait_until(lambda: desktop_status(server)["pairing"])
    result["asked"] = desktop_status(server)["line"]
    with device(server, ip="127.0.0.1") as d:
        d.post("/desktop/pairing", json={"allow": allow})
    thread.join(timeout=10)
    return result["response"], result["asked"]


def pair(server, fakes, ip="127.0.0.2", name="Pixel Watch 4") -> str:
    res, _ = ask_to_pair(server, ip, name, allow=True)
    assert res.status_code == 200, res.text
    return res.json()["token"]


def test_only_clients_trusting_our_ca_can_connect(server):
    with pytest.raises(httpx.ConnectError, match="CERTIFICATE_VERIFY_FAILED"):
        httpx.get(f"{server.url}/discover")  # system trust store only


def test_discover(server):
    with device(server) as d:
        res = d.get("/discover")

    assert res.json() == {"service": "saambha", "name": socket.gethostname()}


def agent_calls(server) -> list[dict]:
    return server.fakes.pi_calls() if server.harness == "pi" else server.fakes.claude_calls()


def session_of(server, call) -> str | None:
    """The session a fake agent run used: pi is given --session-id, Claude Code --resume."""
    flag = "--session-id" if server.harness == "pi" else "--resume"
    return call["args"][call["args"].index(flag) + 1] if flag in call["args"] else None


def test_unpaired_device_is_turned_away(server, fakes):
    with device(server) as d:
        assert d.get("/status").status_code == 401
        assert d.post("/command", json={"text": "hi"}).status_code == 401

    assert agent_calls(server) == []


def test_pair_then_ask(server, fakes):
    res, asked = ask_to_pair(server, "127.0.0.2", "Pixel Watch 4", allow=True)
    token = res.json()["token"]

    assert asked == "Pixel Watch 4 (127.0.0.2) wants to connect"
    with device(server, token=token) as d:
        assert d.get("/status").json()["ok"] is True
        res = d.post("/command", json={"text": "what time is it", "speak": "none"})

    assert res.status_code == 200
    assert res.json()["text"] == "You said: what time is it"
    assert fakes.spoken() == ""  # the phone speaks this reply itself


def test_reply_is_spoken_on_the_desktop(server, fakes):
    token = pair(server, fakes)

    with device(server, token=token) as d:
        d.post("/command", json={"text": "play music"})

    assert wait_until(lambda: fakes.spoken() == "You said: play music")


def test_follow_up_continues_the_conversation(server, fakes):
    token = pair(server, fakes)

    with device(server, token=token) as d:
        d.post("/reset")
        d.post("/command", json={"text": "first", "speak": "none"})
        d.post("/command", json={"text": "second", "speak": "none"})

    first, second = agent_calls(server)
    # pi: saambha picks the id up front; Claude Code: the id comes back from the first run (sess-1).
    expected = session_of(server, first) if server.harness == "pi" else "sess-1"
    assert session_of(server, second) == expected is not None


def test_new_command_cuts_off_the_reply_being_spoken(server, fakes):
    token = pair(server, fakes)
    fakes.set("speech_seconds", 30)

    with device(server, token=token) as d:
        d.post("/command", json={"text": "tell me a long story"})
        assert wait_until(lambda: fakes.spoken() == "You said: tell me a long story")
        speaking = fakes.piper_pid()
        d.post("/command", json={"text": "stop", "speak": "none"})

    assert wait_until(lambda: not is_running(speaking))


def test_denied_device_gets_403_and_is_not_asked_about_again(server, fakes):
    res, _ = ask_to_pair(server, "127.0.0.3", "Stranger", allow=False)
    assert res.status_code == 403

    with device(server, ip="127.0.0.3") as d:
        assert d.post("/pair", json={"name": "Stranger"}).status_code == 403  # straight away

    assert desktop_status(server)["pairing"] is None


def test_revoked_device_is_turned_away(server, fakes):
    token = pair(server, fakes)
    devices = server.work / "devices.json"
    kept = {k: v for k, v in json.loads(devices.read_text()).items() if v["name"] != "Revoke me"}

    revoked = pair(server, fakes, name="Revoke me")
    devices.write_text(json.dumps(kept))

    with device(server, token=revoked) as d:
        assert d.get("/status").status_code == 401
    with device(server, token=token) as d:
        assert d.get("/status").status_code == 200


def test_hotkey_on_this_machine_needs_no_token(server, fakes):
    fakes.set("speech_seconds", 30)

    with device(server, ip="127.0.0.1") as d:
        d.post("/command", json={"text": "hello"})
        assert wait_until(lambda: fakes.spoken() == "You said: hello")
        speaking = fakes.piper_pid()
        assert d.post("/listen").status_code == 200  # the hotkey starting a new recording

    assert wait_until(lambda: not is_running(speaking))
    assert desktop_status(server)["state"] == "listening"
    with device(server, ip="127.0.0.1") as d:
        d.delete("/listen")
    assert desktop_status(server)["state"] == "ready"


def test_desktop_app_sees_what_saambha_is_doing(server, fakes):
    fakes.set("speech_seconds", 30)

    with device(server, ip="127.0.0.1") as d:
        d.post("/command", json={"text": "hello"})
    assert wait_until(lambda: desktop_status(server)["speaking"])

    with device(server, ip="127.0.0.1") as d:
        d.post("/desktop/speech/stop")

    assert wait_until(lambda: not desktop_status(server)["speaking"])
    assert desktop_status(server)["line"] == "Ready"
    with device(server, ip="127.0.0.1") as d:
        assert d.get("/desktop/history").json()[-1]["reply"] == "You said: hello"
        log = d.get("/desktop/log").json()
    assert log["offset"] == len(log["text"].encode())  # all of it: it's shorter than the backlog


def test_server_stops_at_once_while_the_desktop_app_waits(tmp_path):
    """Restarting Saambha mustn't sit out the desktop app's wait for a change."""
    with running_server(tmp_path, "pi") as server:
        seen = desktop_status(server)["version"]

        def wait_for_change():
            with contextlib.suppress(httpx.HTTPError), device(server, ip="127.0.0.1") as d:
                d.get("/desktop/status", params={"after": seen})
        threading.Thread(target=wait_for_change, daemon=True).start()
        time.sleep(0.3)  # it's waiting

        start = time.perf_counter()
        server.proc.terminate()
        server.proc.wait(timeout=20)

    assert time.perf_counter() - start < Settings.default().shutdown_wait + 2


def test_desktop_app_hears_of_a_change_at_once(server, fakes):
    """A benchmark: the dot waits on the status rather than asking over and over, and must still
    light up as soon as the hotkey is pressed."""
    seen = desktop_status(server)["version"]
    waited = {}

    def wait_for_change():
        with device(server, ip="127.0.0.1") as d:
            waited["status"] = d.get("/desktop/status", params={"after": seen}).json()
        waited["at"] = time.perf_counter()
    waiting = threading.Thread(target=wait_for_change)
    waiting.start()
    time.sleep(0.3)  # it's waiting
    assert "status" not in waited

    pressed = time.perf_counter()
    with device(server, ip="127.0.0.1") as d:
        d.post("/listen")
    waiting.join(timeout=5)
    with device(server, ip="127.0.0.1") as d:
        d.delete("/listen")

    assert waited["status"]["state"] == "listening"
    assert waited["at"] - pressed < 0.1


def test_desktop_app_endpoints_turn_away_paired_devices(server, fakes):
    token = pair(server, fakes)

    with device(server, token=token) as d:
        assert d.get("/desktop/status").status_code == 403
        assert d.post("/desktop/pairing", json={"allow": True}).status_code == 403


def touchpad(server, ip="127.0.0.2", token=None):
    """The apps' touchpad connection (a WebSocket) from [ip]. The asyncio client: the sync one
    sometimes misses the handshake's answer over TLS."""
    return connect(server.url.replace("https://", "wss://") + "/mouse", ssl=server.ca,
                   additional_headers={"Authorization": f"Bearer {token}"} if token else {},
                   local_addr=(ip, 0), open_timeout=10)


def test_touchpad_turns_away_unpaired_devices(server):
    async def connect_unpaired():
        async with touchpad(server):
            pass

    with pytest.raises(InvalidStatus, match="403"):
        asyncio.run(connect_unpaired())


def test_touchpad_says_why_it_cannot_move_the_mouse(server, fakes):
    """The server has no /dev/uinput here (serve.py), so this is as far as a test gets without
    moving the real mouse."""
    token = pair(server, fakes)

    async def wait_for_close():
        async with touchpad(server, token=token) as ws:
            await ws.wait_closed()
            return ws.close_code, ws.close_reason

    assert asyncio.run(wait_for_close()) == (1011, "Can't control the mouse: No such file or directory")


@pytest.mark.bench
def test_server_left_alone_costs_next_to_nothing(server, fakes):
    """While the desktop app waits on the status, as it does all day."""
    seen = desktop_status(server)["version"]

    def wait_for_change():
        with device(server, ip="127.0.0.1") as d:
            d.get("/desktop/status", params={"after": seen})
    waiting = threading.Thread(target=wait_for_change)
    waiting.start()
    time.sleep(1)  # settled

    idle = measure(server.pid, seconds=10)
    with device(server, ip="127.0.0.1") as d:  # lets the desktop app's request go
        d.post("/listen")
        d.delete("/listen")
    waiting.join(timeout=5)

    print(f"\n{server.harness}: {idle}")
    assert idle.cpu < 0.01
    assert idle.wakeups < 30
    assert idle.memory_mb < 150
