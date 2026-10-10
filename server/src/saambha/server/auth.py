"""Device pairing: an app calls POST /pair, which asks on the desktop whether to allow it.
Approved devices get a long-lived token they send as `Authorization: Bearer <token>`.
Requests from this machine are always allowed.

Approved devices live in devices.json; delete an entry to revoke it.
"""
import hashlib
import json
import logging
import os
import secrets
import time
from collections.abc import Awaitable, Callable, Mapping
from pathlib import Path

log = logging.getLogger(__name__)

LOOPBACK = {"127.0.0.1", "::1", "::ffff:127.0.0.1"}


def is_local(ip: str) -> bool:
    return ip in LOOPBACK


def bearer_token(headers: Mapping[str, str]) -> str | None:
    auth = headers.get("authorization", "")
    if auth.lower().startswith("bearer "):
        return auth[7:].strip()
    return None


def _hash(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


class DeviceRegistry:
    """Approved devices, keyed by a hash of their token (the token itself isn't stored)."""

    def __init__(self, path: Path):
        self.path = path

    def add(self, name: str, ip: str) -> str:
        token = secrets.token_urlsafe(32)
        devices = self._load()
        devices[_hash(token)] = {"name": name, "ip": ip, "approved_at": time.strftime("%Y-%m-%d %H:%M:%S")}
        self._save(devices)
        return token

    def is_valid(self, token: str | None) -> bool:
        return bool(token) and _hash(token) in self._load()

    def _load(self) -> dict:
        try:
            return json.loads(self.path.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return {}

    def _save(self, devices: dict):
        self.path.write_text(json.dumps(devices, indent=2))
        os.chmod(self.path, 0o600)


class Pairing:
    """Asks on the desktop (one device at a time) and registers the devices that are allowed.

    [ask] puts the question to the user and returns whether it was allowed. A denied IP isn't
    asked again for [deny_cooldown] seconds, so a device can't keep asking.
    """

    def __init__(self, registry: DeviceRegistry, ask: Callable[[str, str], Awaitable[bool]],
                 deny_cooldown: float, clock: Callable[[], float] = time.time):
        self.registry = registry
        self.ask = ask
        self.clock = clock
        self.deny_cooldown = deny_cooldown
        self._asking = False
        self._denied_until: dict[str, float] = {}

    async def request(self, ip: str, device_name: str) -> str | None:
        """Returns a new token, or None if denied. Raises RuntimeError if another device is being asked about."""
        name = device_name[:100] or "unknown"
        if self._denied_until.get(ip, 0) > self.clock():
            return None
        if self._asking:
            raise RuntimeError("Another device is awaiting approval")

        self._asking = True
        try:
            allowed = await self.ask(ip, name)
        finally:
            self._asking = False

        log.info(f"AUTH: {'approved' if allowed else 'denied'} {ip} ({name[:60]})")
        if not allowed:
            self._denied_until[ip] = self.clock() + self.deny_cooldown
            return None
        return self.registry.add(name, ip)
