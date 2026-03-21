#!/usr/bin/env bash
# Toggle recording: first press starts, second press stops and sends

SERVER="https://127.0.0.1:8765"
PID_FILE="/tmp/voice-ai.pid"
TMP_FILE="/tmp/voice-ai-recording.wav"

if [ -f "$PID_FILE" ]; then
    # Already recording — stop and send
    kill "$(cat "$PID_FILE")" 2>/dev/null
    rm -f "$PID_FILE"

    # Unmute speaker
    wpctl set-mute @DEFAULT_AUDIO_SINK@ 0

    if [ -s "$TMP_FILE" ]; then
        zenity --notification --text="⏳ Processing..." &
        if ! curl -sk --fail -X POST "$SERVER/voice" \
            -F "audio=@$TMP_FILE;type=audio/wav" \
            -o /dev/null --max-time 130; then
            zenity --notification --text="Voice AI: Request failed" &
        fi
    else
        zenity --notification --text="Voice AI: No audio captured" &
    fi

    rm -f "$TMP_FILE"
else
    # Not recording — start
    wpctl set-mute @DEFAULT_AUDIO_SINK@ 1
    zenity --notification --text="🎙️ Recording... Press Super+C again to stop" &
    sox -d "$TMP_FILE" &
    echo $! > "$PID_FILE"
fi
