#!/usr/bin/env bash
# gen_certs.sh <dir>: a local CA and a server cert signed by it, in <dir> (each only if missing;
# the server cert also if it no longer verifies, e.g. expired). The apps bundle ca.pem and trust any cert it
# signed, whatever address the server is found at, so keep ca.pem: replacing it means
# rebuilding and reinstalling the apps.
set -euo pipefail
cd "$1"

if [ ! -f ca.pem ] || [ ! -f ca-key.pem ]; then
    echo "Generating local CA..."
    openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
        -keyout ca-key.pem -out ca.pem -subj "/CN=saambha local CA" \
        -addext "basicConstraints=critical,CA:TRUE" \
        -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
fi

if [ -f cert.pem ] && [ -f key.pem ] && openssl verify -CAfile ca.pem cert.pem &>/dev/null; then
    exit 0
fi

HOST="$(hostname).local"
echo "Generating server certificate for $HOST..."
openssl req -newkey rsa:2048 -nodes -keyout key.pem -out cert.csr \
    -subj "/CN=$HOST" 2>/dev/null
openssl x509 -req -in cert.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial \
    -out cert.pem -days 3650 \
    -extfile <(printf "subjectAltName=DNS:%s,DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n" "$HOST") 2>/dev/null
rm -f cert.csr ca.srl
