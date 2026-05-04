#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/docs/demo"
BUILD_DIR="$OUT_DIR/.build-claude-tmux"
DRIVER="$ROOT/scripts/demo/drive_claude_tmux.py"
for tool in asciinema agg ffmpeg python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
done

mkdir -p "$OUT_DIR" "$BUILD_DIR"

render_panel() {
  local mode="$1"
  local cast="$BUILD_DIR/claude-${mode}.cast"
  local gif="$BUILD_DIR/claude-${mode}.gif"

  rm -f "$cast" "$gif"
  asciinema rec \
    --headless \
    --overwrite \
    --quiet \
    --window-size 108x30 \
    --command "python3 $DRIVER $mode" \
    "$cast"

  agg \
    --theme github-dark \
    --cols 108 \
    --rows 30 \
    --font-size 16 \
    --idle-time-limit 0.8 \
    --last-frame-duration 3 \
    "$cast" \
    "$gif" >/dev/null
}

render_panel triseek
render_panel no-triseek

ffmpeg -y \
  -hide_banner \
  -loglevel error \
  -i "$BUILD_DIR/claude-triseek.gif" \
  -i "$BUILD_DIR/claude-no-triseek.gif" \
  -filter_complex "\
[0:v][1:v]hstack=inputs=2,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
  "$OUT_DIR/claude-real-tui-triseek-vs-no-triseek.gif"

echo "$OUT_DIR/claude-real-tui-triseek-vs-no-triseek.gif"
