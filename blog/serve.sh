#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
./build.sh

port="${PORT:-4173}"
echo "blog: serving http://127.0.0.1:${port}/blog/"
python3 -m http.server "${port}" --bind 127.0.0.1 --directory dist
