# Voice AI

Voice-controlled Linux desktop assistant. Speak a command, Claude executes it.

## Setup & Running

Copy `.env.sample` to `.env` and fill in your values, then:

```bash
./start.sh
```

This handles everything: installs system deps (`sox`, `playerctl`), creates a Python venv, installs pip packages, generates SSL certs for mobile access, and starts the server.

### Hotkey

Bind `scripts/voice-ai-trigger.sh` to a keyboard shortcut (e.g. `Super+C`) via Settings → Keyboard → Custom Shortcuts.

### Mobile Access

Open `https://<hostname>.local:8765` on your phone.

## API

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/voice` | Upload audio, transcribe, run |
| POST | `/reset` | Clear session |
| GET | `/status` | Health check |

## Prompts

System prompt is built from `prompts/*.txt` (`base.txt` first, then alphabetically). The AI can add/edit its own prompt files to learn new capabilities.
