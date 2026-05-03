#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <triseek-single|grep-single|triseek-session|grep-session>" >&2
  exit 1
fi

SCENARIO="$1"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPO="$ROOT/../triseek-bench/repos/torvalds_linux"
INDEX="$ROOT/../triseek-bench/indexes/torvalds_linux"
SESSION_QUERY_FILE="$ROOT/scripts/demo/linux-session-20.json"
SESSION_BASELINE_SCRIPT="$ROOT/scripts/demo/linux-session-20-baseline.zsh"
CLI_BIN="$ROOT/target/release/triseek"

cleanup() {
  rm -f "${OUT_FILE:-}" "${TIME_FILE:-}"
}
trap cleanup EXIT

OUT_FILE="$(mktemp)"
TIME_FILE="$(mktemp)"

print_time() {
  sed -n '1,3p' "$TIME_FILE"
}

case "$SCENARIO" in
  triseek-single)
    /usr/bin/time -lp -o "$TIME_FILE" \
      "$CLI_BIN" search \
      --repo "$REPO" \
      --index-dir "$INDEX" \
      --engine auto \
      --kind literal \
      --json \
      --summary-only \
      --max-results 50 \
      AEGIS_BLOCK_SIZE >"$OUT_FILE"

    jq -r '
      "engine=\(.engine) route=\(.routing.selected_engine) files=\(.summary.files_with_matches) matches=\(.summary.total_line_matches) search_ms=\((.metrics.process.wall_millis | floor))"
    ' "$OUT_FILE"
    print_time
    ;;
  grep-single)
    /usr/bin/time -lp -o "$TIME_FILE" \
      rg \
      --json \
      --line-number \
      --color never \
      --no-heading \
      --fixed-strings \
      --max-count 50 \
      AEGIS_BLOCK_SIZE \
      "$REPO" >"$OUT_FILE"

    jq -r '
      select(.type == "summary")
      | "files=\(.data.stats.searches_with_match) matches=\(.data.stats.matched_lines) rg_elapsed=\(.data.elapsed_total.human)"
    ' "$OUT_FILE"
    print_time
    ;;
  triseek-session)
    /usr/bin/time -lp -o "$TIME_FILE" \
      "$CLI_BIN" session \
      --repo "$REPO" \
      --index-dir "$INDEX" \
      --engine auto \
      --query-file "$SESSION_QUERY_FILE" \
      --json \
      --summary-only >"$OUT_FILE"

    jq -r '
      "queries=\(.query_count) engines=\(.engine_counts | to_entries | map("\(.key):\(.value)") | join(",")) total_matches=\(.total_matches) session_ms=\((.metrics.process.wall_millis | floor))"
    ' "$OUT_FILE"
    print_time
    ;;
  grep-session)
    (
      cd "$REPO"
      /usr/bin/time -lp -o "$TIME_FILE" zsh "$SESSION_BASELINE_SCRIPT" >"$OUT_FILE"
    )

    echo "queries=20 backend=rg repeated raw scans"
    print_time
    ;;
  *)
    echo "unknown scenario: $SCENARIO" >&2
    exit 1
    ;;
esac
