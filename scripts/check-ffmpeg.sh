#!/bin/bash

MISSING=0

if ! command -v ffmpeg &>/dev/null; then
  echo "✗ ffmpeg not found"
  MISSING=1
fi

if ! command -v ffprobe &>/dev/null; then
  echo "✗ ffprobe not found"
  MISSING=1
fi

if [ $MISSING -eq 1 ]; then
  echo ""
  echo "Install with:  brew install ffmpeg"
  echo "Or run:        ./scripts/fetch-ffmpeg.sh"
  exit 1
fi

echo "✓ ffmpeg and ffprobe found"
