#!/usr/bin/env bash
cd "$(dirname "$0")"

# Install system dependencies if missing
for cmd in sox playerctl; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "$cmd not found, installing..."
        sudo apt install -y "$cmd"
    fi
done

# Create venv and install Python deps if missing
if [ ! -d "venv" ]; then
    echo "Creating virtual environment..."
    python3 -m venv venv
    source venv/bin/activate
    pip install -r requirements.txt
else
    source venv/bin/activate
fi

# Generate SSL certs for mobile mic access if missing
if [ ! -f "cert.pem" ] || [ ! -f "key.pem" ]; then
    echo "Generating SSL certificates..."
    openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 365 -nodes -subj '/CN=voice-ai'
fi

python3 server.py 2>&1 | tee server.log
