# Voice AI

Voice-controlled Linux desktop assistant. Press a hotkey, speak a command, Claude Code executes it.

## Architecture

```
Super+C (press)  → start recording (sox)
Super+C (press)  → stop recording → POST /voice → Whisper transcribes
                                                 → Claude Code agent loop
                                                 → bash tool executes commands
                                                 → result shown as notification
```

## Setup

### Dependencies

```bash
pip3 install faster-whisper fastapi uvicorn python-multipart anthropic --break-system-packages
sudo apt install sox playerctl
```

### Environment

Copy `.env.example` to `.env` and fill in your values:

| Variable               | Default                      | Description                            |
|------------------------|------------------------------|----------------------------------------|
| `CLAUDE_MODEL`         | `claude-haiku-4-5-20251001`  | Anthropic model for the agent          |
| `WHISPER_MODEL`        | `small`                      | Whisper model (tiny/base/small/medium) |
| `PORT`                 | `8765`                       | Server port                            |
| `SPOTIFY_CLIENT_ID`    | —                            | Spotify API client ID                  |
| `SPOTIFY_CLIENT_SECRET`| —                            | Spotify API client secret              |

### Hotkey Setup

`Super+C` — toggle recording (first press starts, second press stops and sends)

The trigger script is at `scripts/voice-ai-trigger.sh`. To configure the hotkey via gsettings:

```bash
gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings \
  "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/']"

gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/ name 'Voice AI'
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/ command "$HOME/projects/voice-ai/scripts/voice-ai-trigger.sh"
gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/ binding '<Super>c'
```

Or via GUI: **Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts → +**
- Name: `Voice AI`
- Command: `~/projects/voice-ai/scripts/voice-ai-trigger.sh`
- Shortcut: `Super+C`

## Running

```bash
cd ~/projects/voice-ai
python3 server.py
```

Server runs on `http://localhost:8765`

## API Endpoints

| Method | Endpoint   | Description                        |
|--------|------------|------------------------------------|
| POST   | `/voice`   | Upload audio file, transcribe, run |
| POST   | `/text`    | Send text command directly         |
| POST   | `/reset`   | Clear session context              |
| GET    | `/history` | Last 50 commands and results       |
| GET    | `/status`  | Server health info                 |

## How It Works

1. **Trigger** — `voice-ai-trigger.sh` records audio via `sox`, sends it to the server
2. **Transcribe** — `faster-whisper` (local, runs on CPU) converts speech to text
3. **Agent** — Claude Code runs as an agent with a Bash tool, executing commands as needed
4. **Spotify** — `scripts/spotify_play.py` searches the Spotify API for track URIs, then `playerctl` plays them
5. **Notify** — results shown as desktop notifications via `zenity`

