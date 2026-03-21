#!/usr/bin/env bash
cd "$(dirname "$0")"
python3 server.py 2>&1 | tee server.log
