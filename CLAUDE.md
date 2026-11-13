# Saambha

Voice assistant for the Linux desktop. `desktop/` is the app, in Rust, one process: the server (`src/server/`: the HTTPS API the phone, watch and hotkey use, and the agent harness, pi or Claude Code, that runs their commands) and the dot and chat panel on the desktop (`src/ui/`, GTK 4). `agent/` is the agent's workspace (its prompt and guards); `android/` holds the phone and watch apps.

## Keep the code clean

- Only necessary code. No dead code, no handling for situations that can't happen, no settings or options nobody asked for.
- When a change makes something obsolete (code, tests, settings, comments, docs), delete it in the same change. Clean up stale comments and dead code you come across as you go.
- Keep the folder structure clean too: each file in the folder its component and kind belong in, nothing stray at the root, no scratch files, one-off scripts or generated output left behind, and no folder kept once a change empties it or makes it obsolete. A file needed in two places is a symlink, not a copy (`AGENTS.md` links to this file).
- Comments only where the code can't speak for itself: say why, not what. No comments that restate the code or narrate history ("now", "used to", "new").

## Conventions

- Dependency injection: types get their collaborators (programs, paths, clocks, clients, stores) when they're made, behind traits where a test needs a fake. Only the composition roots wire real implementations: `desktop/src/server/compose.rs` (the server), `desktop/src/main.rs` (the app) and, on Android, `Saambha.create(context)`. Tests use fakes, not monkeypatching.
- Settings: every tunable value lives in `desktop/src/server/settings.rs`; each can be overridden per user as `SAAMBHA_<NAME>` in `desktop/.env`, which also holds secrets. Android: the `Settings` data class in `core`.
- Logging: the `log` macros; only `main.rs` configures logging. No `println!` for logging.
- Tests sit with their component and come with every change, ideally first: `desktop/src` (unit, `#[cfg(test)]`, with the fake CLIs in `desktop/tests/fakes`), `desktop/tests` (`e2e.rs` runs the real server over HTTPS; `pi.rs` runs the real pi against a fake model; `models.rs` compares how well the real agent models do the job, on demand with `cargo test --test models -- --run`), `desktop/bench` (the app's idle cost: needs the desktop session and the release build) and `android/core/src/test`.
- Speech runs as programs, each only while it's needed: whisper.cpp for each recording, piper for each reply (`scripts/speech.sh` installs them). The UI calls the server's services directly, in the same process; the API's routes are for the phone, watch, hotkey and agent. The dot's windows float above the others: layer-shell surfaces on Wayland desktops that have one (COSMIC, KDE), X11 windows elsewhere (GNOME, through XWayland). The dot is its own small window, so its animations stay cheap; the full-screen stage below it shows the chat panel and takes clicks only while that's open. Left alone the app must cost next to nothing: the dot is told of changes instead of polling. Its colours live in `src/ui/theme.rs`, which draws the dot with them and hands them to the stylesheet (`src/ui/style.css`) as `@define-color`.
- The user's skills live in their own repo (`SAAMBHA_SKILLS_DIR`), never in this one; tests use `desktop/tests/fixtures/skills`.
- `start.sh` runs `scripts/install.sh` every time, then (re)starts the service, so setup must stay idempotent and quick: each step checks before it acts, so it only installs what's missing, keeps what exists (`.env`, the CA, the speech tools) and applies changes. Before anything needs sudo, say why.
- The README covers setup and usage only.

## Workflow

- Each change on its own branch, merged into `master` by a pull request once CI (`.github/workflows/ci.yml`) passes; `master` is protected, and PRs are squash-merged, so the PR's title and description become the commit. Merged branches are deleted.
- `.githooks/pre-commit` runs the quick checks for what's staged (`start.sh` points git at it). CI runs the rest of "Before finishing" except the benchmarks, which need this machine.

## Before finishing

```bash
cd desktop && cargo test && cargo clippy --all-targets && cargo build --release && node --test bench/*.js
cd android && JAVA_HOME=~/.local/share/jdk17 ANDROID_HOME=~/Android/Sdk ./gradlew :core:testDebugUnitTest
```
