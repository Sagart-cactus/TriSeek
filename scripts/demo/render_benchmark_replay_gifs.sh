#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/docs/demo"
BUILD_DIR="$OUT_DIR/.build"

for tool in asciinema agg ffmpeg jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
done

mkdir -p "$OUT_DIR" "$BUILD_DIR"

render_panel() {
  local client="$1"
  local backend="$2"
  local cast="$BUILD_DIR/${client}-${backend}.cast"
  local gif="$BUILD_DIR/${client}-${backend}.gif"

  rm -f "$cast" "$gif"
  asciinema rec \
    --headless \
    --overwrite \
    --quiet \
    --window-size 108x22 \
    --command "$ROOT/scripts/demo/replay_benchmark_panel.sh $client $backend" \
    "$cast"

  agg \
    --theme github-dark \
    --cols 108 \
    --rows 22 \
    --font-size 16 \
    --idle-time-limit 0.8 \
    --last-frame-duration 2 \
    --speed 1 \
    "$cast" \
    "$gif" >/dev/null
}

stack_gifs() {
  local left="$1"
  local right="$2"
  local out="$3"

  ffmpeg -y \
    -hide_banner \
    -loglevel error \
    -i "$left" \
    -i "$right" \
    -filter_complex "[0:v][1:v]hstack=inputs=2,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
    "$out"
}

for client in claude codex; do
  render_panel "$client" triseek
  render_panel "$client" grep
  stack_gifs \
    "$BUILD_DIR/${client}-triseek.gif" \
    "$BUILD_DIR/${client}-grep.gif" \
    "$OUT_DIR/${client}-triseek-vs-grep.gif"
done

echo "Rendered:"
echo "  $OUT_DIR/claude-triseek-vs-grep.gif"
echo "  $OUT_DIR/codex-triseek-vs-grep.gif"
