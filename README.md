# Voice AI

Voice-controlled Linux desktop assistant. Speak a command, Claude executes it.

## Setup

```bash
sudo apt install sox playerctl
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt
```

Copy `.env.sample` to `.env` and fill in your values.

### Hotkey

Bind `scripts/voice-ai-trigger.sh` to a keyboard shortcut (e.g. `Super+C`) via Settings → Keyboard → Custom Shortcuts.

### Mobile Access

Generate SSL certs for mic access over WiFi:

```bash
openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 365 -nodes -subj '/CN=voice-ai'
```

Then open `https://<hostname>.local:8765` on your phone.

## Running

```bash
python3 server.py
```

## API

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/voice` | Upload audio, transcribe, run |
| POST | `/text` | Send text command |
| POST | `/reset` | Clear session |
| GET | `/status` | Health check |

## Prompts

System prompt is built from `prompts/*.txt` (`base.txt` first, then alphabetically). The AI can add/edit its own prompt files to learn new capabilities.
