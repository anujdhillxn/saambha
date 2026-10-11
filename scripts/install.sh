#!/usr/bin/env bash
# Sets up what Saambha needs on this machine, only what's missing, so start.sh runs it every time.
# Installs what the configured harness needs (SAAMBHA_HARNESS / SAAMBHA_PI_MODEL in .env, see
# src/saambha/settings.py).
set -euo pipefail
cd "$(dirname "$0")/../server"

PI_PACKAGE="@earendil-works/pi-coding-agent@1.1.0"
NODE_MIN="22.19.0"
NODE_DIR="$HOME/.local/share/node22"

step() { printf '\n== %s\n' "$*"; }

step "System packages"
packages=()
reasons=()
need() {  # need <why> <apt package>...
    packages+=("${@:2}")
    reasons+=("  ${*:2}: $1")
}
command -v sox &>/dev/null || need "records the hotkey's voice commands" sox
command -v aplay &>/dev/null || need "plays the spoken replies" alsa-utils
command -v curl &>/dev/null || need "downloads the voice and the harness" curl
command -v openssl &>/dev/null || need "makes the HTTPS certificates" openssl
python3 -c "import ensurepip" &>/dev/null || need "the server's Python environment" python3-venv
command -v cc &>/dev/null || need "building the desktop app" build-essential
pkg-config --exists webkit2gtk-4.1 2>/dev/null || need "building the desktop app (its web view)" libwebkit2gtk-4.1-dev
[ -f /usr/include/xdo.h ] || need "building the desktop app" libxdo-dev
dpkg -s libgtk-layer-shell0 &>/dev/null || need "the desktop app's windows, to float above the others" libgtk-layer-shell0

if [ ${#packages[@]} -gt 0 ]; then
    echo "Installing with apt, which needs your password (sudo):"
    printf '%s\n' "${reasons[@]}"
    sudo apt-get install -y "${packages[@]}"
else
    echo "all installed"
fi

step "Python environment"
[ -d venv ] || python3 -m venv venv
if ! cmp -s requirements.txt venv/requirements.txt; then  # the copy is what was last installed
    venv/bin/pip install -q -r requirements.txt
    cp requirements.txt venv/requirements.txt
fi
echo "ok"

step ".env"
if [ -f .env ]; then
    echo "exists"
else
    cp .env.sample .env
    echo "created from .env.sample: add your secrets and settings"
fi

step "Git hooks (the quick checks before each commit)"
if [ "$(git -C .. config core.hooksPath)" = .githooks ]; then
    echo "set"
else
    git -C .. config core.hooksPath .githooks
    echo "set up"
fi

step "HTTPS certificates"
../scripts/gen_certs.sh .
echo "ok"

step "Mouse (the apps' touchpad moves a virtual one)"
if [ -w /dev/uinput ]; then
    echo "ok"
else
    echo "Making virtual input devices (/dev/uinput) takes a udev rule that allows it for whoever is logged in,"
    echo "which needs your password (sudo)"
    echo 'KERNEL=="uinput", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/70-saambha-uinput.rules >/dev/null
    sudo udevadm control --reload
    sudo udevadm trigger --name-match=uinput
    echo "allowed"
fi

# The settings the rest depends on, as the server will see them.
eval "$(PYTHONPATH=src venv/bin/python - <<'PY'
import shlex
from saambha.main import load_settings
s = load_settings()
for name in ("port", "harness", "pi_model", "ollama_url", "ollama_context", "skills_dir"):
    print(f"{name.upper()}={shlex.quote(str(getattr(s, name)))}")
PY
)"

step "Skills ($SKILLS_DIR)"
if [ -d "$SKILLS_DIR" ]; then
    echo "found"
else
    mkdir -p "$SKILLS_DIR"
    echo "created (empty): put your skills repo here, or set SAAMBHA_SKILLS_DIR in .env"
fi

node_ok() {  # is `node` at least $NODE_MIN?
    command -v node &>/dev/null &&
        [ "$(printf '%s\n%s\n' "$NODE_MIN" "$(node --version | tr -d v)" | sort -V | head -1)" = "$NODE_MIN" ]
}

install_node22() {  # latest Node 22 LTS into $NODE_DIR, checksum-verified
    local version file tmp
    version=$(curl -fsSL https://nodejs.org/dist/index.json |
        python3 -c "import json,sys; print(next(r['version'] for r in json.load(sys.stdin) if r['version'].startswith('v22.') and r['lts']))")
    file="node-$version-linux-x64.tar.xz"
    tmp=$(mktemp -d)
    curl -fsSL -o "$tmp/$file" "https://nodejs.org/dist/$version/$file"
    curl -fsSL -o "$tmp/SHASUMS256.txt" "https://nodejs.org/dist/$version/SHASUMS256.txt"
    (cd "$tmp" && grep " $file\$" SHASUMS256.txt | sha256sum -c -)
    mkdir -p "$NODE_DIR"
    tar -xJf "$tmp/$file" -C "$NODE_DIR" --strip-components=1
    rm -rf "$tmp"
}

setup_pi() {
    step "pi"
    if command -v pi &>/dev/null; then
        echo "found $(pi --version)"
    else
        mkdir -p "$HOME/.local/bin"
        if node_ok; then
            npm install -g --prefix "$HOME/.local" --ignore-scripts "$PI_PACKAGE"
        else
            [ -x "$NODE_DIR/bin/node" ] || install_node22
            PATH="$NODE_DIR/bin:$PATH" "$NODE_DIR/bin/npm" install -g --ignore-scripts "$PI_PACKAGE"
            # pi needs Node >= $NODE_MIN, newer than this system's, so run it on our own.
            cat > "$HOME/.local/bin/pi" <<WRAPPER
#!/bin/sh
PATH="$NODE_DIR/bin:\$PATH" exec "$NODE_DIR/bin/node" "$NODE_DIR/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js" "\$@"
WRAPPER
            chmod +x "$HOME/.local/bin/pi"
        fi
        echo "installed $("$HOME/.local/bin/pi" --version)"
    fi

    case "$PI_MODEL" in
        ollama/*) setup_ollama "${PI_MODEL#ollama/}" ;;
        *) echo "Log pi in to the model's provider once: run \`pi\`, then /login" ;;
    esac
}

setup_ollama() {  # $1: the model pi uses, e.g. saambha-gemma4:e4b
    local model=$1 base=${1#saambha-}
    step "Ollama ($model)"
    if ! command -v ollama &>/dev/null; then
        echo "Installing Ollama with its official script, which needs your password (sudo) to install it system-wide"
        curl -fsSL https://ollama.com/install.sh | sh
    fi
    ollama show "$base" &>/dev/null || ollama pull "$base"
    if [ "$model" != "$base" ] && ! ollama show "$model" --parameters 2>/dev/null | grep -qE "^num_ctx +$OLLAMA_CONTEXT\$"; then
        local modelfile
        modelfile=$(mktemp)
        printf 'FROM %s\nPARAMETER num_ctx %s\n' "$base" "$OLLAMA_CONTEXT" > "$modelfile"
        ollama create "$model" -f "$modelfile"
        rm "$modelfile"
    fi
    # Tell pi about it: provider "ollama" in ~/.pi/agent/models.json (other entries are kept).
    PYTHONPATH=src venv/bin/python - "$model" "$OLLAMA_URL" "$OLLAMA_CONTEXT" <<'PY'
import json, sys
from pathlib import Path
from saambha.agent.pi import with_ollama_model
path = Path.home() / ".pi" / "agent" / "models.json"
config = json.loads(path.read_text()) if path.exists() else {}
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(with_ollama_model(config, sys.argv[2], sys.argv[1], int(sys.argv[3])), indent=2) + "\n")
PY
    echo "ok"
}

setup_claude() {
    step "Claude Code"
    if command -v claude &>/dev/null; then
        echo "found $(claude --version)"
    else
        curl -fsSL https://claude.ai/install.sh | bash
        echo "Log in once: run \`claude\`, then /login"
    fi
}

case "$HARNESS" in
    pi) setup_pi ;;
    claude) setup_claude ;;
    *) echo "Unknown harness '$HARNESS' (SAAMBHA_HARNESS): use pi or claude" >&2; exit 1 ;;
esac

step "Desktop app (the dot and its chat panel)"
if ! command -v cargo &>/dev/null && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
    echo "Installing Rust (rustup, into ~/.cargo) to build it"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --component clippy --no-modify-path
fi
(cd ../desktop && PATH="$HOME/.cargo/bin:$PATH" cargo build --release)
echo "ok"

service() {  # service <name> <description> <command> [more [Service] lines]: a user service that runs while you're logged in
    cat > "$HOME/.config/systemd/user/$1.service" <<UNIT
[Unit]
Description=$2
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=$3
Restart=on-failure
${4:-}

[Install]
WantedBy=graphical-session.target
UNIT
}

step "Background services"
mkdir -p "$HOME/.config/systemd/user"
service saambha "Saambha voice assistant" "$PWD/venv/bin/python -m saambha" "WorkingDirectory=$PWD
Environment=PYTHONPATH=$PWD/src"
service saambha-desktop "Saambha's dot and chat panel" \
    "$(cd ../desktop && pwd)/target/release/saambha-desktop https://127.0.0.1:$PORT $PWD/ca.pem"
systemctl --user daemon-reload
systemctl --user enable --quiet saambha saambha-desktop
echo "ok"
