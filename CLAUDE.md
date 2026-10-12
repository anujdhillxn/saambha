# Saambha

Voice assistant for the Linux desktop. `server/` (Python, FastAPI) runs commands through an agent harness (pi or Claude Code); `desktop/` (Tauri: Rust and plain HTML/CSS/JS) is the dot and chat panel on the desktop; `android/` holds the phone and watch apps.

## Keep the code clean

- Only necessary code. No dead code, no handling for situations that can't happen, no settings or options nobody asked for.
- When a change makes something obsolete (code, tests, settings, comments, docs), delete it in the same change. Clean up stale comments and dead code you come across as you go.
- Keep the folder structure clean too: each file in the folder its component and kind belong in, nothing stray at the root, no scratch files, one-off scripts or generated output left behind, and no folder kept once a change empties it or makes it obsolete. A file needed in two places is a symlink, not a copy (`AGENTS.md` links to this file).
- Comments only where the code can't speak for itself: say why, not what. No comments that restate the code or narrate history ("now", "used to", "new").

## Conventions

- Dependency injection: classes get their collaborators (programs, paths, clocks, clients, stores) through the constructor. Only the composition roots wire real implementations: `server/src/saambha/main.py` and, on Android, `Saambha.create(context)`. Tests use fakes, not monkeypatching.
- Settings: every tunable value lives in `server/src/saambha/settings.py`; each can be overridden per user as `SAAMBHA_<NAME>` in `server/.env`, which also holds secrets. Android: the `Settings` data class in `core`.
- Logging: `logging.getLogger(__name__)`; only the composition root configures logging. No `print`.
- Tests sit with their component and come with every change, ideally first: `server/tests` (unit; `e2e/` runs the real server, with benchmarks of what it uses marked `bench`; `integration/` runs the real pi against a fake model; `models/` compares how well the real agent models do the job, on demand with `pytest -m models`), `desktop/src` (Rust `#[cfg(test)]`), `desktop/tests` (the UI's logic, `node --test`), `desktop/bench` (the app's idle cost: needs the desktop session, the release build and the server) and `android/core/src/test`.
- `desktop/` talks to the server over HTTPS through its Rust side (`/desktop/*` answers this machine only). Its windows float above the others: layer-shell surfaces on Wayland desktops that have one (COSMIC, KDE), X11 windows elsewhere (GNOME, through XWayland). The dot is its own small window, so its animations stay cheap; the full-screen stage below it shows the chat panel and takes clicks only while that's open. Left alone it must cost next to nothing: it waits on the server for changes instead of polling. Styles use the custom properties in `ui/shared/theme.css`.
- The user's skills live in their own repo (`SAAMBHA_SKILLS_DIR`), never in this one; tests use `server/tests/fixtures/skills`.
- `start.sh` runs `scripts/install.sh` every time, then (re)starts the services, so setup must stay idempotent and quick: each step checks before it acts, so it only installs what's missing, keeps what exists (`.env`, the CA, the venv) and applies changes. Before anything needs sudo, say why.
- The README covers setup and usage only.

## Workflow

- Each change on its own branch, merged into `master` by a pull request once CI (`.github/workflows/ci.yml`) passes; `master` is protected, and PRs are squash-merged, so the PR's title and description become the commit. Merged branches are deleted.
- `.githooks/pre-commit` runs the quick checks for what's staged (`start.sh` points git at it). CI runs the rest of "Before finishing" except the benchmarks, which need this machine.

## Before finishing

```bash
cd server && venv/bin/ruff check src tests && venv/bin/pytest && venv/bin/pytest -m bench
cd desktop && cargo test && cargo clippy && node --test && cargo build --release && node --test bench/*.js
cd android && JAVA_HOME=~/.local/share/jdk17 ANDROID_HOME=~/Android/Sdk ./gradlew :core:testDebugUnitTest
```
