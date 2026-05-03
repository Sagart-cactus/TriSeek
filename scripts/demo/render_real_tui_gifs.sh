#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/docs/demo"
BUILD_DIR="$OUT_DIR/.build-real-tui"
DRIVER="$ROOT/scripts/demo/drive_real_tui.py"
FONT="/System/Library/Fonts/Menlo.ttc"

for tool in asciinema agg ffmpeg python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
done

mkdir -p "$OUT_DIR" "$BUILD_DIR"

render_panel() {
  local client="$1"
  local mode="$2"
  local cast="$BUILD_DIR/${client}-${mode}.cast"
  local gif="$BUILD_DIR/${client}-${mode}.gif"

  rm -f "$cast" "$gif"
  asciinema rec \
    --headless \
    --overwrite \
    --quiet \
    --window-size 108x30 \
    --command "python3 $DRIVER $client $mode" \
    "$cast"

  agg \
    --theme github-dark \
    --cols 108 \
    --rows 30 \
    --font-size 16 \
    --idle-time-limit 0.8 \
    --last-frame-duration 3 \
    --speed 1 \
    "$cast" \
    "$gif" >/dev/null
}

stack_gifs() {
  local left="$1"
  local right="$2"
  local out="$3"
  local left_title="$4"
  local right_title="$5"

  ffmpeg -y \
    -hide_banner \
    -loglevel error \
    -i "$left" \
    -i "$right" \
    -filter_complex "\
[0:v]tpad=stop_mode=clone:stop_duration=8,pad=iw:ih+34:0:34:color=#111111,drawtext=fontfile=$FONT:text='$left_title':x=14:y=8:fontsize=20:fontcolor=#86efac[left];\
[1:v]tpad=stop_mode=clone:stop_duration=8,pad=iw:ih+34:0:34:color=#111111,drawtext=fontfile=$FONT:text='$right_title':x=14:y=8:fontsize=20:fontcolor=#fca5a5[right];\
[left][right]hstack=inputs=2,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
    "$out"
}

for client in claude codex; do
  render_panel "$client" triseek
  render_panel "$client" no-triseek
  stack_gifs \
    "$BUILD_DIR/${client}-triseek.gif" \
    "$BUILD_DIR/${client}-no-triseek.gif" \
    "$OUT_DIR/${client}-real-tui-triseek-vs-no-triseek.gif" \
    "TriSeek MCP Installed" \
    "No TriSeek MCP"
done

echo "Rendered:"
echo "  $OUT_DIR/claude-real-tui-triseek-vs-no-triseek.gif"
echo "  $OUT_DIR/codex-real-tui-triseek-vs-no-triseek.gif"
