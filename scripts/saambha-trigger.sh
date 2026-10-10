#!/usr/bin/env bash
# Toggle recording: first press starts, second press stops and sends.
# The Saambha dot shows it listening, then working on the command.

SERVER="https://127.0.0.1:8765"
PID_FILE="/tmp/saambha.pid"
TMP_FILE="/tmp/saambha-recording.wav"

if [ -f "$PID_FILE" ]; then
    # Already recording — stop and send
    kill "$(cat "$PID_FILE")" 2>/dev/null
    rm -f "$PID_FILE"

    # Unmute speaker
    wpctl set-mute @DEFAULT_AUDIO_SINK@ 0

    if [ -s "$TMP_FILE" ]; then
        curl -sk -X POST "$SERVER/voice" -F "audio=@$TMP_FILE;type=audio/wav" -o /dev/null --max-time 130
    else
        curl -sk -X DELETE "$SERVER/listen" -o /dev/null --max-time 2
    fi

    rm -f "$TMP_FILE"
else
    # Not recording — start (the server cuts off any reply still being spoken)
    curl -sk -X POST "$SERVER/listen" -o /dev/null --max-time 2 &
    wpctl set-mute @DEFAULT_AUDIO_SINK@ 1
    sox -d "$TMP_FILE" &
    echo $! > "$PID_FILE"
fi
