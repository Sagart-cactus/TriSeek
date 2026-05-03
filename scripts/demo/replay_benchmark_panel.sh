#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <claude|codex> <triseek|grep>" >&2
  exit 1
fi

CLIENT="$1"
BACKEND="$2"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPORT="$ROOT/bench/results/rerun-2026-04-02-all/report.json"
SESSION_QUERY_FILE="$ROOT/scripts/demo/linux-session-20.json"
SESSION_BASELINE_SCRIPT="$ROOT/scripts/demo/linux-session-20-baseline.zsh"

fmt_ms() {
  awk -v n="$1" 'BEGIN { printf "%.0f ms", n }'
}

fmt_ratio() {
  awk -v a="$1" -v b="$2" 'BEGIN { if (b == 0) print "n/a"; else printf "%.1fx", a / b }'
}

single_triseek_ms="$(jq -r '.case_reports[] | select(.repo_slug=="torvalds_linux" and .query.label=="literal_selective") | .indexed.aggregate.p50_wall_millis' "$REPORT")"
single_grep_ms="$(jq -r '.case_reports[] | select(.repo_slug=="torvalds_linux" and .query.label=="literal_selective") | .baseline.aggregate.p50_wall_millis' "$REPORT")"
session_triseek_ms="$(jq -r '.session_reports[] | select(.repo_slug=="torvalds_linux" and .label=="session_20") | .indexed.aggregate.p50_wall_millis' "$REPORT")"
session_grep_ms="$(jq -r '.session_reports[] | select(.repo_slug=="torvalds_linux" and .label=="session_20") | .baseline.aggregate.p50_wall_millis' "$REPORT")"

single_fast_ratio="$(fmt_ratio "$single_grep_ms" "$single_triseek_ms")"
session_fast_ratio="$(fmt_ratio "$session_grep_ms" "$session_triseek_ms")"

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

reset=$'\033[0m'
dim=$'\033[2m'
bold=$'\033[1m'
blue=$'\033[38;5;39m'
green=$'\033[38;5;42m'
red=$'\033[38;5;203m'
yellow=$'\033[38;5;221m'

if [[ "$BACKEND" == "triseek" ]]; then
  accent="$green"
  backend_name="TriSeek MCP"
  single_cmd="triseek search --repo ../TriSeek-bench/repos/torvalds_linux --index-dir ../TriSeek-bench/indexes/torvalds_linux --engine auto --kind literal --json --summary-only --max-results 50 AEGIS_BLOCK_SIZE"
  session_cmd="triseek session --repo ../TriSeek-bench/repos/torvalds_linux --index-dir ../TriSeek-bench/indexes/torvalds_linux --engine auto --query-file scripts/demo/linux-session-20.json --json --summary-only"
  single_ms="$single_triseek_ms"
  session_ms="$session_triseek_ms"
  single_rel="$single_fast_ratio faster than rg"
  session_rel="$session_fast_ratio faster than grep loop"
  single_summary="summary: 3 files, 50 line matches, indexed path"
  session_summary="summary: same 20 searches, p50 from rerun benchmark"
else
  accent="$red"
  backend_name="shell grep"
  single_cmd="rg --json --line-number --color never --no-heading --fixed-strings --max-count 50 AEGIS_BLOCK_SIZE ../TriSeek-bench/repos/torvalds_linux"
  session_cmd="(cd ../TriSeek-bench/repos/torvalds_linux && zsh scripts/demo/linux-session-20-baseline.zsh)"
  single_ms="$single_grep_ms"
  session_ms="$session_grep_ms"
  single_rel="$single_fast_ratio slower than TriSeek"
  session_rel="$session_fast_ratio slower than TriSeek session"
  single_summary="summary: same query, same 3 files, same 50 line matches"
  session_summary="summary: same 20 searches, repeated raw rg scans"
fi

clear_screen() {
  printf '\033c'
}

pause() {
  sleep "$1"
}

print_header() {
  printf '%b%s%b\n' "$accent$bold" "$client_name + $backend_name" "$reset"
  printf '%b%s%b\n' "$dim" "Source: bench/results/rerun-2026-04-02-all" "$reset"
  printf '%b%s%b\n' "$dim" "Repo: torvalds/linux" "$reset"
  printf '\n'
}

show_scene() {
  local scene_title="$1"
  local prompt_line="$2"
  local cmd_line="$3"
  local summary_line="$4"
  local millis="$5"
  local relative_line="$6"

  clear_screen
  print_header
  printf '%b%s%b\n\n' "$blue$bold" "$scene_title" "$reset"
  printf '%bPrompt%b\n' "$yellow" "$reset"
  printf '  %s\n\n' "$prompt_line"
  pause 0.6
  printf '%b$ %s%b\n' "$bold" "$cmd_line" "$reset"
  pause 1.0
  printf '%b%s%b\n' "$dim" "$summary_line" "$reset"
  pause 0.5
  printf '\n%bbenchmark p50:%b %s\n' "$yellow" "$reset" "$(fmt_ms "$millis")"
  printf '%brelative:%b %s\n' "$yellow" "$reset" "$relative_line"
  pause 2.0
}

show_scene "Scene 1/2: single lookup" "$single_prompt" "$single_cmd" "$single_summary" "$single_ms" "$single_rel"
show_scene "Scene 2/2: repeated search session" "$session_prompt" "$session_cmd" "$session_summary" "$session_ms" "$session_rel"

clear_screen
  print_header
printf '%bTakeaway%b\n' "$blue$bold" "$reset"
printf '  Same repo.\n'
printf '  Same benchmark source.\n'
printf '  TriSeek wins on medium+ repos once the agent keeps searching.\n\n'
printf '%bsingle-query p50%b  TriSeek %s  vs  grep %s\n' "$yellow" "$reset" "$(fmt_ms "$single_triseek_ms")" "$(fmt_ms "$single_grep_ms")"
printf '%bsession_20 p50%b   TriSeek %s  vs  grep %s\n' "$yellow" "$reset" "$(fmt_ms "$session_triseek_ms")" "$(fmt_ms "$session_grep_ms")"
pause 3.0
