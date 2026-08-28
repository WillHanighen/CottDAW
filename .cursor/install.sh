#!/usr/bin/env bash
# CottDAW Cloud Agent install: system libraries, Rust toolchain, then build.
# Idempotent and non-interactive; safe to run repeatedly.
set -euo pipefail

cd "$(dirname "$0")/.."

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  SUDO="sudo"
fi

# --- System libraries -------------------------------------------------------
# PipeWire/ALSA (cpal), X11 + OpenGL (eframe/egui + x11-dl, VST editor embed),
# LV2 (lilv), plus cmake/clang/pkg-config for native builds, ffmpeg for Opus /
# Gonio MP4 export, and Xvfb + mesa software GL so the GUI can run headless.
export DEBIAN_FRONTEND=noninteractive
$SUDO apt-get update -y
$SUDO apt-get install -y --no-install-recommends \
  build-essential pkg-config cmake clang libclang-dev \
  libasound2-dev libpipewire-0.3-dev libspa-0.2-dev liblilv-dev \
  libx11-dev libx11-xcb-dev libxcb1-dev libxcursor-dev libxrandr-dev \
  libxi-dev libxext-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libgl1-mesa-dev libegl1-mesa-dev libglu1-mesa-dev libgl1-mesa-dri \
  ffmpeg xvfb x11-utils

# --- Rust toolchain ---------------------------------------------------------
# The workspace is edition 2024, which requires rustc >= 1.85.
if command -v rustup >/dev/null 2>&1; then
  rustup toolchain install stable --profile minimal --no-self-update
  rustup default stable
else
  echo "rustup not found; ensure a Rust toolchain >= 1.85 is installed" >&2
  exit 1
fi

rustc --version

# --- Build ------------------------------------------------------------------
# Bundles the first-party VST3s (CottSynth/CottFilter/CottVinyl) then builds
# the cott-daw host and cott-vst-worker binaries into target/debug/.
./scripts/build-daw.sh
