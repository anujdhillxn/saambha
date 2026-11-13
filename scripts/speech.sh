#!/usr/bin/env bash
# Speech for Saambha, each piece only if it's missing: whisper.cpp's CLI (built from source) and its
# model, which turn the hotkey's recordings into commands, and piper and its voice, which speak the
# replies. Everything is checked against the checksum it was pinned with.
set -euo pipefail
DIR="$HOME/.local/share/saambha"

WHISPER_VERSION=1.9.5
WHISPER_SHA256=ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720
WHISPER_MODEL=ggml-small-q8_0.bin  # whisper "small", quantized: as good, at half the size
WHISPER_MODEL_SHA256=49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f
PIPER_RELEASE=2023.11.14-2
PIPER_SHA256=a50cb45f355b7af1f6d758c1b360717877ba0a398cc8cbe6d2a7a3a26e225992
VOICES="$HOME/.local/share/piper-voices"
VOICE_URL=https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/lessac/medium/en_US-lessac-medium
VOICE_SHA256=5efe09e69902187827af646e1a6e9d269dee769f9877d17b16b1b46eeaaf019f
VOICE_CONFIG_SHA256=efe19c417bed055f2d69908248c6ba650fa135bc868b0e6abb3da181dab690a0

fetch() {  # fetch <url> <sha256> <file>: downloads <url> to <file> if it's what was pinned
    local tmp
    tmp=$(mktemp)
    curl -fsSL -o "$tmp" "$1"
    echo "$2  $tmp" | sha256sum -c --quiet -
    mkdir -p "$(dirname "$3")"
    mv "$tmp" "$3"
}

if [ ! -x "$DIR/whisper/whisper-cli" ]; then
    echo "Building whisper.cpp $WHISPER_VERSION..."
    tmp=$(mktemp -d)
    fetch "https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v$WHISPER_VERSION.tar.gz" "$WHISPER_SHA256" "$tmp/whisper.tar.gz"
    tar -xzf "$tmp/whisper.tar.gz" -C "$tmp"
    cmake -S "$tmp/whisper.cpp-$WHISPER_VERSION" -B "$tmp/build" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
        -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_SERVER=OFF > /dev/null
    cmake --build "$tmp/build" -j "$(nproc)" --target whisper-cli > /dev/null
    mkdir -p "$DIR/whisper"
    cp "$tmp/build/bin/whisper-cli" "$DIR/whisper/"
    rm -rf "$tmp"
fi
[ -f "$DIR/whisper/$WHISPER_MODEL" ] ||
    fetch "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$WHISPER_MODEL" "$WHISPER_MODEL_SHA256" "$DIR/whisper/$WHISPER_MODEL"

if [ ! -x "$DIR/piper/piper" ]; then
    tmp=$(mktemp -d)
    fetch "https://github.com/rhasspy/piper/releases/download/$PIPER_RELEASE/piper_linux_x86_64.tar.gz" "$PIPER_SHA256" "$tmp/piper.tar.gz"
    tar -xzf "$tmp/piper.tar.gz" -C "$DIR"  # into piper/
    rm -rf "$tmp"
fi
[ -f "$VOICES/en_US-lessac-medium.onnx" ] || fetch "$VOICE_URL.onnx" "$VOICE_SHA256" "$VOICES/en_US-lessac-medium.onnx"
[ -f "$VOICES/en_US-lessac-medium.onnx.json" ] ||
    fetch "$VOICE_URL.onnx.json" "$VOICE_CONFIG_SHA256" "$VOICES/en_US-lessac-medium.onnx.json"
