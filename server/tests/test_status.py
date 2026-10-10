import asyncio
import threading
import time

import pytest

from conftest import wait_until
from saambha.server.status import Activity, Status


@pytest.mark.parametrize("activity, state, line", [
    (Activity(), "ready", "Ready"),
    (Activity(model_cold="Loading the model"), "cold", "Loading the model"),
    (Activity(model_cold="Paused: steam is using 6.0 GB of GPU memory", speaking=True), "working", "Speaking"),
    (Activity(task="Running skill spotify", speaking=True), "working", "Running skill spotify"),
    (Activity(listening=True, task="Thinking"), "listening", "Listening"),
    (Activity(listening=True, pairing="Phone (192.168.0.12)"), "asking", "Phone (192.168.0.12) wants to connect"),
])
def test_most_pressing_thing_shows(activity, state, line):
    assert activity.look == (state, line)


def test_snapshot():
    status = Status(approval_timeout=5, change_wait=5)
    status.set_model_cold("Loading the model")
    status.set_speaking(True)

    assert status.snapshot() == {
        "state": "working", "line": "Speaking", "model_cold": "Loading the model", "listening": False,
        "task": None, "speaking": True, "pairing": None, "version": 2,
    }


def test_next_is_at_once_when_already_changed():
    status = Status(approval_timeout=5, change_wait=5)
    status.set_listening(True)

    assert asyncio.run(status.next(after=0))["state"] == "listening"


def test_next_after_a_version_from_before_a_restart_is_at_once():
    status = Status(approval_timeout=5, change_wait=5)

    assert asyncio.run(status.next(after=57))["version"] == 0


def test_next_waits_for_a_change():
    status = Status(approval_timeout=5, change_wait=5)
    threading.Timer(0.05, status.set_task, ["Thinking"]).start()

    snapshot = asyncio.run(status.next(after=0))

    assert (snapshot["state"], snapshot["version"]) == ("working", 1)


def test_next_with_no_change_is_as_it_is_after_the_wait():
    status = Status(approval_timeout=5, change_wait=0.05)

    assert asyncio.run(status.next(after=0)) == status.snapshot()


def test_a_change_reaches_a_waiting_next_within_milliseconds():
    status = Status(approval_timeout=5, change_wait=5)
    changed_at = []

    def change():
        changed_at.append(time.perf_counter())
        status.set_listening(True)

    threading.Timer(0.05, change).start()
    asyncio.run(status.next(after=0))

    assert time.perf_counter() - changed_at[0] < 0.005


def test_pairing_is_a_change():
    status = Status(approval_timeout=5, change_wait=5)
    asked = ask_in_background(status)

    assert asyncio.run(status.next(after=0))["state"] == "asking"
    status.answer(True)
    asked["thread"].join(timeout=5)


def ask_in_background(status, ip="192.168.0.12", name="Pixel Watch 4") -> dict:
    result = {}
    thread = threading.Thread(target=lambda: result.update(allowed=asyncio.run(status.ask(ip, name))))
    thread.start()
    result["thread"] = thread
    return result


@pytest.mark.parametrize("allowed", [True, False])
def test_device_is_allowed_or_denied(allowed):
    status = Status(approval_timeout=5, change_wait=5)
    asked = ask_in_background(status)
    assert wait_until(lambda: status.activity.pairing == "Pixel Watch 4 (192.168.0.12)")

    assert status.answer(allowed) is True

    asked["thread"].join(timeout=5)
    assert asked["allowed"] is allowed
    assert status.activity.pairing is None


def test_unanswered_device_is_denied():
    status = Status(approval_timeout=0.1, change_wait=5)

    assert asyncio.run(status.ask("192.168.0.12", "Phone")) is False
    assert status.activity.pairing is None


def test_answer_with_nobody_waiting():
    assert Status(approval_timeout=5, change_wait=5).answer(True) is False
