#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
if [ ! -d .build-venv ]; then
  python3 -m venv .build-venv
fi
PY=.build-venv/bin/python
"$PY" -m pip install --upgrade pip
"$PY" -m pip install -r requirements-build.txt
"$PY" -m PyInstaller --noconfirm --clean tgws.spec
echo "Sidecar built: $(pwd)/dist/tgws-proxy"
