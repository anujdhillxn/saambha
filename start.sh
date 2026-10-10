#!/usr/bin/env bash
# Sets up whatever is missing, then (re)starts Saambha in the background.
set -euo pipefail
cd "$(dirname "$0")"
scripts/install.sh
systemctl --user restart saambha saambha-desktop
echo
echo "Saambha is running, and starts when you log in (status: systemctl --user status saambha saambha-desktop)."
