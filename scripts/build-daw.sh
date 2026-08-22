#!/usr/bin/env bash
# Bundle built-in VSTs, then build the DAW + worker.
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE="${1:-debug}"
if [[ "$PROFILE" == "release" ]]; then
  cargo bundle-synth
  cargo bundle-filter
  cargo bundle-whistle
  cargo bundle-haze
  cargo bundle-vinyl
  cargo bundle-tape
  cargo bundle-bass
  cargo bundle-pluck
  cargo bundle-kit
  cargo build --release -p cott-daw -p cott-vst-worker
else
  cargo bundle-synth-debug
  cargo bundle-filter-debug
  cargo bundle-whistle-debug
  cargo bundle-haze-debug
  cargo bundle-vinyl-debug
  cargo bundle-tape-debug
  cargo bundle-bass-debug
  cargo bundle-pluck-debug
  cargo bundle-kit-debug
  cargo build -p cott-daw -p cott-vst-worker
fi
