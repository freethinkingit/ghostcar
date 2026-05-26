#!/bin/bash
set -e

# For local dev: builds ffmpeg from source (arm64) or uses cached binaries
# In CI, build-ffmpeg.sh is used directly with GitHub Actions cache

BINARIES_DIR="src-tauri/binaries"
TARGET="aarch64-apple-darwin"

if [ -x "$BINARIES_DIR/ffmpeg-$TARGET" ] && [ -x "$BINARIES_DIR/ffprobe-$TARGET" ]; then
  echo "ffmpeg binaries already present, skipping."
  file "$BINARIES_DIR/ffmpeg-$TARGET"
  exit 0
fi

echo "Building ffmpeg from source (arm64)..."
echo "This takes ~5 minutes the first time."
./scripts/build-ffmpeg.sh
