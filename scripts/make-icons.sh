#!/bin/sh
# Render platform icon files from the standalone brand symbol. Needs resvg and Python 3.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
python3 scripts/make-icons.py
