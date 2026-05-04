#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="$ROOT/docs/demo"
BUILD_DIR="$OUT_DIR/.build-live"
REPO="$ROOT/../triseek-bench/repos/torvalds_linux"
INDEX="$ROOT/../triseek-bench/indexes/torvalds_linux"
CLI_BIN="$ROOT/target/release/triseek"
SERVER_BIN="$ROOT/target/release/triseek-server"
SESSION_QUERY_FILE="$ROOT/scripts/demo/linux-session-20.json"
SESSION_BASELINE_SCRIPT="$ROOT/scripts/demo/linux-session-20-baseline.zsh"
SERVER_LOG="$BUILD_DIR/triseek-server.log"
SERVER_PID_FILE="$BUILD_DIR/triseek-server.pid"

for tool in asciinema agg ffmpeg jq rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
done

if [[ ! -x "$CLI_BIN" ]]; then
  echo "missing required binary: $CLI_BIN" >&2
  exit 1
fi

if [[ ! -x "$SERVER_BIN" ]]; then
  echo "missing required binary: $SERVER_BIN" >&2
  exit 1
fi

mkdir -p "$OUT_DIR" "$BUILD_DIR"

cleanup() {
  if [[ -f "$SERVER_PID_FILE" ]]; then
    kill "$(cat "$SERVER_PID_FILE")" >/dev/null 2>&1 || true
    rm -f "$SERVER_PID_FILE"
  fi
}
trap cleanup EXIT

start_server() {
  rm -f "$SERVER_LOG"
  nohup "$SERVER_BIN" \
    --repo "$REPO" \
    --index-dir "$INDEX" \
    --idle-timeout 600 >"$SERVER_LOG" 2>&1 &
  echo $! >"$SERVER_PID_FILE"

  for _ in $(seq 1 50); do
    if "$CLI_BIN" search \
      --repo "$REPO" \
      --index-dir "$INDEX" \
      --engine auto \
      --kind literal \
      --json \
      --summary-only \
      --max-results 50 \
      AEGIS_BLOCK_SIZE >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.2
  done

  echo "failed to warm triseek-server" >&2
  if [[ -f "$SERVER_LOG" ]]; then
    sed -n '1,120p' "$SERVER_LOG" >&2
  fi
  exit 1
}

warm_up() {
  "$CLI_BIN" search \
    --repo "$REPO" \
    --index-dir "$INDEX" \
    --engine auto \
    --kind literal \
    --json \
    --summary-only \
    --max-results 50 \
    AEGIS_BLOCK_SIZE >/dev/null

  rg \
    --json \
    --line-number \
    --color never \
    --no-heading \
    --fixed-strings \
    --max-count 50 \
    AEGIS_BLOCK_SIZE \
    "$REPO" >/dev/null

  "$CLI_BIN" session \
    --repo "$REPO" \
    --index-dir "$INDEX" \
    --engine auto \
    --query-file "$SESSION_QUERY_FILE" \
    --json \
    --summary-only >/dev/null

  (
    cd "$REPO"
    zsh "$SESSION_BASELINE_SCRIPT" >/dev/null
  )
}

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
    --command "$ROOT/scripts/demo/live_benchmark_panel.sh $client $backend" \
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

start_server
warm_up

for client in claude codex; do
  render_panel "$client" triseek
  render_panel "$client" grep
  stack_gifs \
    "$BUILD_DIR/${client}-triseek.gif" \
    "$BUILD_DIR/${client}-grep.gif" \
    "$OUT_DIR/${client}-triseek-vs-grep-live.gif"
done

echo "Rendered:"
echo "  $OUT_DIR/claude-triseek-vs-grep-live.gif"
echo "  $OUT_DIR/codex-triseek-vs-grep-live.gif"
