# Saambha

Voice-controlled Linux desktop assistant. Speak a command, an AI agent executes it.

```
server/    the desktop server (Python): src/saambha, tests, and the agent's workspace (system-prompt.md)
desktop/   the dot and chat panel on the desktop (Tauri)
android/   the phone and watch apps (Kotlin): core, mobile, wear
scripts/   setup (start.sh runs it), the HTTPS certificates, and the hotkey
```

## Setup

```bash
./start.sh   # sets up whatever is missing (system packages, Python env, .env, certificates, what the harness needs, the desktop app), then starts Saambha
```

Saambha then runs in the background (the `saambha` and `saambha-desktop` systemd user services) and starts when you log in. A small microphone dot floats above your windows (drag it anywhere), grey while the local model is cold; clicking it opens the chat, with the detailed logs a button away.

Put secrets and your settings in `server/.env` (see `server/.env.sample`). Any setting in `server/src/saambha/settings.py` can be overridden there as `SAAMBHA_<NAME>`. After changing them, run `./start.sh` again. To run the server in a terminal instead: `systemctl --user stop saambha`, then `cd server && PYTHONPATH=src venv/bin/python -m saambha`.

## Harness

Commands run through an agent harness, `SAAMBHA_HARNESS`:

- `claude` (default): [Claude Code](https://claude.com/claude-code). Log in once: run `claude`, then `/login`.
- `pi`: [pi](https://pi.dev), with `SAAMBHA_PI_MODEL`. For a hosted model, log in once: run `pi`, then `/login`. For a local one, use `ollama/saambha-<model>` (e.g. `ollama/saambha-gemma4:e4b`): `start.sh` sets it up in Ollama, and the server unloads it while something heavy (a game, a render) runs.

## Hotkey

Bind `scripts/saambha-trigger.sh` to a keyboard shortcut (e.g. `Super+C`).

## Phone and watch apps

Build (needs JDK 17 + Android SDK, and `server/ca.pem` from `start.sh`):

```bash
cd android && ./gradlew assembleDebug
```

Install over wireless debugging (Developer options → Wireless debugging → Pair new device):

```bash
adb pair <ip>:<pairing-port>
adb connect <ip>:<port>
adb install -r android/mobile/build/outputs/apk/debug/mobile-debug.apk   # or wear/.../wear-debug.apk
```

The app finds the server on your Wi-Fi when it opens. The first time, click **Allow** in the popup that opens on the desktop. To unpair a device, remove it from `server/devices.json`.

## Skills

Skills live in their own git repo, not in this one: one folder per skill, each with a `SKILL.md` (frontmatter with `name` and `description`, then instructions) and any scripts it needs. To connect yours, clone it and point `SAAMBHA_SKILLS_DIR` in `server/.env` at it (default: `~/saambha-skills`), then run `./start.sh` again.

Rules that apply to every request go in `server/system-prompt.md`.

## Tests

```bash
cd server && venv/bin/pip install -r requirements-dev.txt && venv/bin/ruff check src tests && venv/bin/pytest
cd desktop && cargo test && node --test
cd android && ./gradlew :core:testDebugUnitTest
```
