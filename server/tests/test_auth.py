import asyncio
import json

import pytest

from saambha.server.auth import Pairing, bearer_token, is_local


def answers(*replies: bool):
    """A fake desktop question that gives [replies] in order and records who was asked about."""
    asked = []

    async def ask(ip, name):
        asked.append((ip, name))
        return replies[len(asked) - 1]
    ask.asked = asked
    return ask


# DeviceRegistry

def test_tokens_are_stored_hashed(registry):
    token = registry.add("Phone", "192.168.0.12")

    assert registry.is_valid(token)
    assert token not in registry.path.read_text()
    assert oct(registry.path.stat().st_mode & 0o777) == "0o600"


def test_unknown_and_missing_tokens_are_invalid(registry):
    registry.add("Phone", "192.168.0.12")

    assert not registry.is_valid("not-a-token")
    assert not registry.is_valid(None)
    assert not registry.is_valid("")


def test_deleting_a_device_revokes_its_token(registry):
    keep = registry.add("Phone", "192.168.0.12")
    revoke = registry.add("Watch", "192.168.0.11")
    devices = json.loads(registry.path.read_text())
    registry.path.write_text(json.dumps({k: v for k, v in devices.items() if v["name"] != "Watch"}))

    assert registry.is_valid(keep)
    assert not registry.is_valid(revoke)


def test_no_devices_file_yet(registry):
    assert not registry.is_valid("anything")


# Request helpers

@pytest.mark.parametrize("ip", ["127.0.0.1", "::1", "::ffff:127.0.0.1"])
def test_this_machine_is_local(ip):
    assert is_local(ip)


@pytest.mark.parametrize("ip", ["192.168.0.12", "127.0.0.2", ""])
def test_other_addresses_are_not_local(ip):
    assert not is_local(ip)


@pytest.mark.parametrize("header, token", [
    ("Bearer abc123", "abc123"),
    ("bearer abc123", "abc123"),
    ("Basic abc123", None),
    ("", None),
])
def test_bearer_token(header, token):
    assert bearer_token({"authorization": header} if header else {}) == token


# Pairing

def test_allowed_device_is_registered(registry, clock):
    ask = answers(True)
    pairing = Pairing(registry, ask, deny_cooldown=300, clock=clock)

    token = asyncio.run(pairing.request("192.168.0.12", "Pixel Watch 4"))

    assert registry.is_valid(token)
    assert ask.asked == [("192.168.0.12", "Pixel Watch 4")]


def test_long_device_names_are_cut_short(registry, clock):
    ask = answers(True)

    asyncio.run(Pairing(registry, ask, deny_cooldown=300, clock=clock).request("192.168.0.12", "x" * 500))

    assert ask.asked == [("192.168.0.12", "x" * 100)]


def test_denied_device_is_not_asked_again_until_the_cooldown_ends(registry, clock):
    ask = answers(False, True)
    pairing = Pairing(registry, ask, deny_cooldown=300, clock=clock)

    assert asyncio.run(pairing.request("192.168.0.12", "Phone")) is None
    clock.advance(299)
    assert asyncio.run(pairing.request("192.168.0.12", "Phone")) is None
    assert len(ask.asked) == 1

    clock.advance(2)
    assert registry.is_valid(asyncio.run(pairing.request("192.168.0.12", "Phone")))


def test_cooldown_is_per_device(registry, clock):
    pairing = Pairing(registry, answers(False, True), deny_cooldown=300, clock=clock)

    asyncio.run(pairing.request("192.168.0.12", "Phone"))

    assert registry.is_valid(asyncio.run(pairing.request("192.168.0.20", "Watch")))


def test_one_question_at_a_time(registry, clock):
    async def slow_yes(ip, name):
        await asyncio.sleep(0.1)
        return True
    pairing = Pairing(registry, slow_yes, deny_cooldown=300, clock=clock)

    async def two_devices():
        return await asyncio.gather(
            pairing.request("192.168.0.12", "Phone"),
            pairing.request("192.168.0.20", "Watch"),
            return_exceptions=True,
        )
    first, second = asyncio.run(two_devices())

    assert registry.is_valid(first)
    assert isinstance(second, RuntimeError)
    # Once the first question is answered, the next device can ask.
    assert registry.is_valid(asyncio.run(pairing.request("192.168.0.20", "Watch")))


def test_failure_to_ask_does_not_block_later_pairing(registry, clock):
    calls = []

    async def flaky(ip, name):
        calls.append(ip)
        if len(calls) == 1:
            raise OSError("D-Bus is gone")
        return True
    pairing = Pairing(registry, flaky, deny_cooldown=300, clock=clock)

    with pytest.raises(OSError):
        asyncio.run(pairing.request("192.168.0.12", "Phone"))

    assert registry.is_valid(asyncio.run(pairing.request("192.168.0.12", "Phone")))
