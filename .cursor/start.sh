#!/usr/bin/env bash
# CottDAW Cloud Agent start: bring up a headless X server on :99 so the egui
# GUI (cott-daw) and native VST3 editors can run. Idempotent; returns promptly.
set -euo pipefail

# Reuse an existing server if one is already up.
if xdpyinfo -display :99 >/dev/null 2>&1; then
  echo "Xvfb already running on :99"
  exit 0
fi

# Software OpenGL (llvmpipe) since Cloud Agent VMs have no GPU.
Xvfb :99 -screen 0 1400x900x24 >/tmp/xvfb.log 2>&1 &

# Wait briefly for the display to come up so agents can use it immediately.
for _ in $(seq 1 25); do
  if xdpyinfo -display :99 >/dev/null 2>&1; then
    echo "Xvfb ready on :99"
    exit 0
  fi
  sleep 0.2
done

echo "Xvfb failed to start; see /tmp/xvfb.log" >&2
exit 1
