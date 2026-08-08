#!/usr/bin/env bash
# Bundle built-in VSTs, then build the DAW + worker.
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE="${1:-debug}"
if [[ "$PROFILE" == "release" ]]; then
  cargo bundle-synth
  cargo bundle-filter
  cargo build --release -p cott-daw -p cott-vst-worker
else
  cargo bundle-synth-debug
  cargo bundle-filter-debug
  cargo build -p cott-daw -p cott-vst-worker
fi
