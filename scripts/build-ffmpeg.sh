#!/bin/bash
set -e

# Builds static arm64 ffmpeg + ffprobe for macOS Apple Silicon
# Used in CI; cached so it only runs once per FFMPEG_VERSION bump

FFMPEG_VERSION="${FFMPEG_VERSION:-7.1.1}"
PREFIX="$(pwd)/ffmpeg-build"
OUTPUT_DIR="${1:-src-tauri/binaries}"
TARGET="aarch64-apple-darwin"

mkdir -p "$PREFIX" "$OUTPUT_DIR"

export MACOSX_DEPLOYMENT_TARGET=11.0
export CFLAGS="-arch arm64"
export LDFLAGS="-arch arm64"

cd /tmp

# Download ffmpeg source
if [ ! -d "ffmpeg-${FFMPEG_VERSION}" ]; then
  curl -L "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" -o ffmpeg.tar.xz
  tar xf ffmpeg.tar.xz
fi

cd "ffmpeg-${FFMPEG_VERSION}"

./configure \
  --prefix="$PREFIX" \
  --arch=arm64 \
  --enable-cross-compile \
  --target-os=darwin \
  --enable-videotoolbox \
  --enable-audiotoolbox \
  --enable-static \
  --disable-shared \
  --disable-doc \
  --disable-debug \
  --disable-ffplay \
  --disable-network \
  --disable-autodetect \
  --enable-hwaccel=h264_videotoolbox \
  --enable-hwaccel=hevc_videotoolbox \
  --enable-encoder=h264_videotoolbox \
  --enable-encoder=hevc_videotoolbox \
  --enable-demuxer=mov,mp4,matroska,mxf \
  --enable-muxer=mp4 \
  --enable-decoder=h264,hevc,prores,pcm_s16le,pcm_s24le,aac \
  --enable-encoder=aac \
  --enable-protocol=file \
  --enable-filter=scale,format \
  --extra-cflags="-arch arm64" \
  --extra-ldflags="-arch arm64"

make -j$(sysctl -n hw.ncpu)

cp ffmpeg "$OUTPUT_DIR/ffmpeg-$TARGET"
cp ffprobe "$OUTPUT_DIR/ffprobe-$TARGET"
chmod +x "$OUTPUT_DIR/ffmpeg-$TARGET" "$OUTPUT_DIR/ffprobe-$TARGET"

# Symlinks for PATH discovery
ln -sf "ffmpeg-$TARGET" "$OUTPUT_DIR/ffmpeg"
ln -sf "ffprobe-$TARGET" "$OUTPUT_DIR/ffprobe"

echo "Built ffmpeg $FFMPEG_VERSION (arm64) → $OUTPUT_DIR"
file "$OUTPUT_DIR/ffmpeg-$TARGET"
