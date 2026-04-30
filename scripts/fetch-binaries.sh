#!/usr/bin/env bash
# Fetch sidecar binaries (yt-dlp, ffmpeg, whisper-cli) into src-tauri/binaries/
# with the target-triple suffix Tauri expects. Run once after cloning, and any
# time you want to refresh a binary (yt-dlp ages fast — YouTube-side breakage
# happens monthly).
#
# Usage:
#   scripts/fetch-binaries.sh                      # auto-detect host
#   scripts/fetch-binaries.sh x86_64-apple-darwin
#   scripts/fetch-binaries.sh aarch64-apple-darwin
#   scripts/fetch-binaries.sh x86_64-pc-windows-msvc
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$ROOT/src-tauri/binaries"
mkdir -p "$BIN_DIR"

TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
  TARGET=$(rustc -vV | awk '/^host:/ {print $2}')
fi

echo "==> target: $TARGET"
echo "==> output: $BIN_DIR"

case "$TARGET" in
  x86_64-apple-darwin|aarch64-apple-darwin)
    EXT=""
    YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    ;;
  x86_64-pc-windows-msvc)
    EXT=".exe"
    YT_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    ;;
  *)
    echo "Unsupported target: $TARGET" >&2
    exit 1
    ;;
esac

YT_OUT="$BIN_DIR/yt-dlp-$TARGET$EXT"
FF_OUT="$BIN_DIR/ffmpeg-$TARGET$EXT"
WS_OUT="$BIN_DIR/whisper-cli-$TARGET$EXT"

# yt-dlp — official standalone binary
echo "==> yt-dlp"
curl -L --fail --silent --show-error -o "$YT_OUT" "$YT_URL"
chmod +x "$YT_OUT" 2>/dev/null || true

# ffmpeg + whisper-cli — host-specific sources
case "$TARGET" in
  *-apple-darwin)
    if ! command -v brew >/dev/null; then
      echo "brew is required to fetch ffmpeg + whisper-cli on macOS" >&2
      exit 1
    fi
    echo "==> ffmpeg (brew)"
    brew list ffmpeg >/dev/null 2>&1 || brew install ffmpeg
    cp "$(brew --prefix)/bin/ffmpeg" "$FF_OUT"
    chmod +x "$FF_OUT"
    echo "==> whisper-cli (brew)"
    brew list whisper-cpp >/dev/null 2>&1 || brew install whisper-cpp
    cp "$(brew --prefix)/bin/whisper-cli" "$WS_OUT"
    chmod +x "$WS_OUT"
    ;;
  x86_64-pc-windows-msvc)
    # On Windows we expect the CI workflow to handle this — see
    # .github/workflows/build.yml. This script is mainly for local Mac dev.
    echo "Windows target: please use CI or fetch ffmpeg.exe and whisper-cli.exe manually." >&2
    echo "  ffmpeg:      https://www.gyan.dev/ffmpeg/builds/ (extract bin/ffmpeg.exe)" >&2
    echo "  whisper-cli: https://github.com/ggerganov/whisper.cpp/releases/latest" >&2
    ;;
esac

# macOS: strip quarantine attr so unsigned binaries can run during dev
if [[ "$TARGET" == *apple-darwin ]]; then
  xattr -dr com.apple.quarantine "$BIN_DIR" 2>/dev/null || true
fi

echo "==> done"
ls -la "$BIN_DIR"
