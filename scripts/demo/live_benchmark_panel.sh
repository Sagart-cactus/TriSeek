#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <claude|codex> <triseek|grep>" >&2
  exit 1
fi

CLIENT="$1"
BACKEND="$2"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MEASURE_SCRIPT="$ROOT/scripts/demo/live_measure.sh"

case "$CLIENT" in
  claude)
    client_name="Claude Code"
    single_prompt="Find AEGIS_BLOCK_SIZE in torvalds/linux."
    session_prompt="Keep searching the same Linux repo through a 20-query debugging session."
    ;;
  codex)
    client_name="Codex"
    single_prompt="Locate AEGIS_BLOCK_SIZE in torvalds/linux."
    session_prompt="Continue with 20 follow-up repo searches in the same Linux session."
    ;;
  *)
    echo "unknown client: $CLIENT" >&2
    exit 1
    ;;
esac

case "$BACKEND" in
  triseek)
    accent=$'\033[38;5;42m'
    backend_name="TriSeek live"
    setup_note_1="persistent triseek-server started before recording"
    setup_note_2="filesystem cache warmed with one dry-run search and one dry-run session"
    single_cmd="target/release/triseek search --repo ../triseek-bench/repos/torvalds_linux --index-dir ../triseek-bench/indexes/torvalds_linux --engine auto --kind literal --json --summary-only --max-results 50 AEGIS_BLOCK_SIZE"
    session_cmd="target/release/triseek session --repo ../triseek-bench/repos/torvalds_linux --index-dir ../triseek-bench/indexes/torvalds_linux --engine auto --query-file scripts/demo/linux-session-20.json --json --summary-only"
    single_scenario="triseek-single"
    session_scenario="triseek-session"
    ;;
  grep)
    accent=$'\033[38;5;203m'
    backend_name="grep live"
    setup_note_1="filesystem cache warmed with one dry-run single lookup"
    setup_note_2="filesystem cache warmed with one dry-run 20-query rg loop"
    single_cmd="rg --json --line-number --color never --no-heading --fixed-strings --max-count 50 AEGIS_BLOCK_SIZE ../triseek-bench/repos/torvalds_linux"
    session_cmd="cd ../triseek-bench/repos/torvalds_linux && zsh /Users/trivedi/Documents/Projects/TriSeek/scripts/demo/linux-session-20-baseline.zsh"
    single_scenario="grep-single"
    session_scenario="grep-session"
    ;;
  *)
    echo "unknown backend: $BACKEND" >&2
    exit 1
    ;;
esac

reset=$'\033[0m'
dim=$'\033[2m'
bold=$'\033[1m'
blue=$'\033[38;5;39m'
yellow=$'\033[38;5;221m'

clear_screen() {
  printf '\033c'
}

pause() {
  sleep "$1"
}

print_header() {
  printf '%b%s%b\n' "$accent$bold" "$client_name + $backend_name" "$reset"
  printf '%b%s%b\n' "$dim" "Repo: torvalds/linux" "$reset"
  printf '%b%s%b\n' "$dim" "Capture: true live run on warmed setup" "$reset"
  printf '\n'
}

show_setup() {
  clear_screen
  print_header
  printf '%bSetup%b\n' "$blue$bold" "$reset"
  printf '  %s\n' "$setup_note_1"
  printf '  %s\n' "$setup_note_2"
  pause 2.0
}

show_scene() {
  local scene_title="$1"
  local prompt_line="$2"
  local cmd_line="$3"
  local scenario="$4"

  clear_screen
  print_header
  printf '%b%s%b\n\n' "$blue$bold" "$scene_title" "$reset"
  printf '%bPrompt%b\n' "$yellow" "$reset"
  printf '  %s\n\n' "$prompt_line"
  pause 0.6
  printf '%b$ %s%b\n' "$bold" "$cmd_line" "$reset"
  pause 0.4
  "$MEASURE_SCRIPT" "$scenario"
  pause 2.0
}

show_setup
show_scene "Scene 1/2: single lookup" "$single_prompt" "$single_cmd" "$single_scenario"
show_scene "Scene 2/2: repeated search session" "$session_prompt" "$session_cmd" "$session_scenario"

clear_screen
print_header
printf '%bTakeaway%b\n' "$blue$bold" "$reset"
printf '  Same repo.\n'
printf '  Same warmed machine.\n'
printf '  Actual wall-clock output from this checkout.\n'
pause 3.0
