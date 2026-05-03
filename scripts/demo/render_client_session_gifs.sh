#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/docs/demo"
BUILD_DIR="$OUT_DIR/.build-client-session"

for tool in asciinema agg ffmpeg jq rg python3; do
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
    --window-size 90x20 \
    --command "$ROOT/scripts/demo/client_session_panel.sh $client $mode" \
    "$cast"

  agg \
    --theme github-dark \
    --cols 90 \
    --rows 20 \
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
  render_panel "$client" no-triseek
  stack_gifs \
    "$BUILD_DIR/${client}-triseek.gif" \
    "$BUILD_DIR/${client}-no-triseek.gif" \
    "$OUT_DIR/${client}-cli-triseek-vs-no-triseek.gif"
done

echo "Rendered:"
echo "  $OUT_DIR/claude-cli-triseek-vs-no-triseek.gif"
echo "  $OUT_DIR/codex-cli-triseek-vs-no-triseek.gif"
