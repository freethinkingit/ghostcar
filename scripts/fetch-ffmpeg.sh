#!/bin/bash
set -e

BINARIES_DIR="src-tauri/binaries"
TARGET="aarch64-apple-darwin"

mkdir -p "$BINARIES_DIR"

echo "Downloading ffmpeg..."
curl -L "https://evermeet.cx/ffmpeg/ffmpeg-7.1.1.zip" -o /tmp/ffmpeg.zip
unzip -o /tmp/ffmpeg.zip -d /tmp
mv /tmp/ffmpeg "$BINARIES_DIR/ffmpeg-$TARGET"
chmod +x "$BINARIES_DIR/ffmpeg-$TARGET"

echo "Downloading ffprobe..."
curl -L "https://evermeet.cx/ffmpeg/ffprobe-7.1.1.zip" -o /tmp/ffprobe.zip
unzip -o /tmp/ffprobe.zip -d /tmp
mv /tmp/ffprobe "$BINARIES_DIR/ffprobe-$TARGET"
chmod +x "$BINARIES_DIR/ffprobe-$TARGET"

rm -f /tmp/ffmpeg.zip /tmp/ffprobe.zip

echo "Done: $BINARIES_DIR"
ls -lh "$BINARIES_DIR"
