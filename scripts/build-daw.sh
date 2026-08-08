#!/usr/bin/env bash
# Bundle CottSynth, then build the DAW + worker.
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE="${1:-debug}"
if [[ "$PROFILE" == "release" ]]; then
  cargo bundle-synth
  cargo build --release -p cott-daw -p cott-vst-worker
else
  cargo bundle-synth-debug
  cargo build -p cott-daw -p cott-vst-worker
fi
