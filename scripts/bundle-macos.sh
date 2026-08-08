#!/usr/bin/env bash
# Build universal (arm64 + x86_64) CottSynth / CottFilter VST3s on macOS.
# Usage: ./scripts/bundle-macos.sh [outdir]
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS (use GitHub Actions workflow macos-vst3.yml from Linux)." >&2
  exit 1
fi

OUTDIR="${1:-$HOME/Downloads/cott-vst3-macos}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-10.13}"

rustup target add aarch64-apple-darwin x86_64-apple-darwin

cargo run -p cott-xtask --release -- bundle-universal \
  -p cott-synth -p cott-filter --release

mkdir -p "$OUTDIR"
rm -rf "$OUTDIR/cott-synth.vst3" "$OUTDIR/cott-filter.vst3"
cp -a target/bundled/cott-synth.vst3 "$OUTDIR/"
cp -a target/bundled/cott-filter.vst3 "$OUTDIR/"
cp -a scripts/INSTALL-macOS-VST3.txt "$OUTDIR/"

for bundle in "$OUTDIR"/*.vst3; do
  codesign --force --deep -s - "$bundle" || true
done

echo "Bundled → $OUTDIR"
ls -la "$OUTDIR"
