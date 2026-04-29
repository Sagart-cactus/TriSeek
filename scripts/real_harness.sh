#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

if [[ -z "${TRISEEK_HARNESS_IN_DOCKER:-}" ]]; then
  echo "real_harness: this harness is intended to run in Docker." >&2
  echo "Run: scripts/run_real_harness_docker.sh" >&2
  exit 2
fi

timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
RESULTS_DIR="${TRISEEK_HARNESS_RESULTS_DIR:-${ROOT}/validation_results/real-harness-${timestamp}}"
LOG_DIR="${RESULTS_DIR}/logs"
ARTIFACT_DIR="${RESULTS_DIR}/artifacts"
mkdir -p "${LOG_DIR}" "${ARTIFACT_DIR}"

WORK_DIR="$(mktemp -d /tmp/triseek-real-harness.XXXXXX)"
HARNESS_HOME="${WORK_DIR}/home"
SMALL_REPO="${WORK_DIR}/small-repo"
QUERY_FILE="${WORK_DIR}/queries.json"

export HOME="${HARNESS_HOME}/user"
export USERPROFILE="${HOME}"
export XDG_CONFIG_HOME="${HARNESS_HOME}/config"
export TRISEEK_HOME="${HARNESS_HOME}/state"
export NO_COLOR=1
export PATH="/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin:${PATH:-}"

BIN="${CARGO_TARGET_DIR:-${ROOT}/target}/release/triseek"
SERVER_BIN="${CARGO_TARGET_DIR:-${ROOT}/target}/release/triseek-server"

MCP_IN=""
MCP_OUT=""
MCP_PID=""
MCP_STDIN_FIFO=""
MCP_STDOUT_FIFO=""

log() {
  printf 'real_harness: %s\n' "$*"
}

fail() {
  printf 'real_harness: FAIL: %s\n' "$*" >&2
  exit 1
}

run_logged() {
  local name="$1"
  shift
  log "$name"
  "$@" >"${LOG_DIR}/${name}.stdout" 2>"${LOG_DIR}/${name}.stderr"
}

assert_file() {
  [[ -f "$1" ]] || fail "expected file: $1"
}

assert_grep() {
  local pattern="$1"
  local file="$2"
  grep -Eq "$pattern" "$file" || fail "expected ${file} to match ${pattern}"
}

assert_json() {
  local filter="$1"
  local file="$2"
  jq -e "$filter" "$file" >/dev/null || fail "json assertion failed for ${file}: ${filter}"
}

cleanup() {
  set +e
  if [[ -n "${MCP_PID}" ]]; then
    kill "${MCP_PID}" >/dev/null 2>&1 || true
    wait "${MCP_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -x "${BIN}" ]]; then
    "${BIN}" daemon stop >/dev/null 2>&1 || true
  fi
  rm -rf "${WORK_DIR}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

create_small_repo() {
  mkdir -p "${SMALL_REPO}/src" "${SMALL_REPO}/docs" "${SMALL_REPO}/tests"
  cat >"${SMALL_REPO}/src/auth.rs" <<'EOF'
pub struct AuthConfig {
    pub timeout_secs: u64,
}

pub fn load_auth_config() -> AuthConfig {
    AuthConfig { timeout_secs: 30 }
}

pub fn validate_service_account() {
    panic!("auth panic");
}
EOF
  cat >"${SMALL_REPO}/src/router.rs" <<'EOF'
pub fn route_request(path: &str) -> &'static str {
    if path.contains("auth") {
        "auth"
    } else {
        "public"
    }
}
EOF
  cat >"${SMALL_REPO}/tests/auth_test.rs" <<'EOF'
#[test]
fn auth_config_fixture_mentions_authconfig() {
    let marker = "AuthConfig";
    assert_eq!(marker, "AuthConfig");
}
EOF
  cat >"${SMALL_REPO}/docs/hooks.md" <<'EOF'
# Hooks

TriSeek validates Claude, Codex, OpenCode, and Pi install surfaces.
EOF
  cat >"${SMALL_REPO}/README.md" <<'EOF'
# Small TriSeek Harness Fixture

Search for AuthConfig and route_request in this deterministic repository.
EOF
  cat >"${SMALL_REPO}/.gitignore" <<'EOF'
ignored/
EOF
  mkdir -p "${SMALL_REPO}/ignored"
  echo "AuthConfig should not be indexed here" >"${SMALL_REPO}/ignored/secret.txt"
  git init -q "${SMALL_REPO}"

  cat >"${QUERY_FILE}" <<'EOF'
[
  {
    "name": "auth-config",
    "kind": "literal",
    "engine": "auto",
    "pattern": "AuthConfig",
    "case_mode": "sensitive",
    "path_substrings": [],
    "path_prefixes": [],
    "exact_paths": [],
    "exact_names": [],
    "extensions": [],
    "globs": [],
    "include_hidden": false,
    "include_binary": false,
    "max_results": 20
  },
  {
    "name": "rust-files",
    "kind": "path",
    "engine": "auto",
    "pattern": ".rs",
    "case_mode": "sensitive",
    "path_substrings": [],
    "path_prefixes": [],
    "exact_paths": [],
    "exact_names": [],
    "extensions": [],
    "globs": [],
    "include_hidden": false,
    "include_binary": false,
    "max_results": 20
  }
]
EOF
}

build_binaries() {
  log "cargo-build-release"
  cargo build --release --locked --bin triseek --bin triseek-server \
    >"${LOG_DIR}/cargo-build-release.stdout" \
    2>"${LOG_DIR}/cargo-build-release.stderr"
  [[ -x "${BIN}" ]] || fail "missing triseek binary at ${BIN}"
  [[ -x "${SERVER_BIN}" ]] || fail "missing triseek-server binary at ${SERVER_BIN}"
}

run_cli_checks() {
  run_logged cli-help "${BIN}" help
  run_logged cli-build-json "${BIN}" build --json "${SMALL_REPO}"
  assert_json '.action == "build" and (.metadata.repo_stats.searchable_files >= 4)' "${LOG_DIR}/cli-build-json.stdout"

  run_logged cli-update "${BIN}" update "${SMALL_REPO}"
  assert_json '.action == "update" and (.metadata.repo_stats.searchable_files >= 4)' "${LOG_DIR}/cli-update.stdout"

  run_logged cli-measure "${BIN}" measure "${SMALL_REPO}"
  assert_json '.searchable_files >= 4' "${LOG_DIR}/cli-measure.stdout"

  run_logged cli-search-index "${BIN}" search --no-daemon --json --engine index AuthConfig "${SMALL_REPO}"
  assert_json '.summary.files_with_matches >= 2 and any(.. | objects; ((.path? // "") | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/cli-search-index.stdout"

  run_logged cli-context-pack-json "${BIN}" context-pack --json --goal "fix auth panic for service accounts" --intent bugfix --budget-tokens 1200 --max-files 4 "${SMALL_REPO}"
  assert_json '.version == "1" and .intent == "bugfix" and .budget_tokens <= 1200 and (.items | length >= 1 and length <= 4) and any(.items[]; (.path | sub("^\\./"; "")) == "src/auth.rs") and all(.items[]; has("content") | not)' "${LOG_DIR}/cli-context-pack-json.stdout"

  run_logged cli-search-after-context-pack "${BIN}" search --no-daemon --json --engine index AuthConfig "${SMALL_REPO}"
  assert_json '.summary.files_with_matches >= 2 and any(.. | objects; ((.path? // "") | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/cli-search-after-context-pack.stdout"

  run_logged cli-search-scan "${BIN}" search --no-daemon --json --engine scan AuthConfig "${SMALL_REPO}"
  assert_json '.summary.files_with_matches >= 2 and any(.. | objects; ((.path? // "") | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/cli-search-scan.stdout"

  run_logged cli-search-rg "${BIN}" search --no-daemon --json --engine rg AuthConfig "${SMALL_REPO}"
  assert_json '.summary.files_with_matches >= 2 and any(.. | objects; ((.path? // "") | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/cli-search-rg.stdout"

  run_logged cli-search-path "${BIN}" search --no-daemon --json --kind path auth "${SMALL_REPO}"
  assert_json '.summary.files_with_matches >= 1 and any(.. | objects; ((.path? // "") | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/cli-search-path.stdout"

  run_logged cli-session "${BIN}" session --json --query-file "${QUERY_FILE}" "${SMALL_REPO}"
  assert_json '.query_count == 2 and .total_matches >= 1 and (.results | length == 2)' "${LOG_DIR}/cli-session.stdout"
}

start_daemon() {
  run_logged daemon-start "${BIN}" daemon start --idle-timeout 0 "${SMALL_REPO}"
  assert_file "${TRISEEK_HOME}/daemon/daemon.port"
  run_logged daemon-status "${BIN}" daemon status "${SMALL_REPO}"
  assert_json '.root.index_available == true and .active_roots >= 1' "${LOG_DIR}/daemon-status.stdout"
}

mcp_start() {
  log "mcp-stdio-start"
  MCP_STDIN_FIFO="${WORK_DIR}/mcp.stdin"
  MCP_STDOUT_FIFO="${WORK_DIR}/mcp.stdout"
  mkfifo "${MCP_STDIN_FIFO}" "${MCP_STDOUT_FIFO}"
  "${BIN}" mcp serve --repo "${SMALL_REPO}" --index-dir "${SMALL_REPO}/.triseek-index" \
    <"${MCP_STDIN_FIFO}" >"${MCP_STDOUT_FIFO}" 2>"${LOG_DIR}/mcp-server.stderr" &
  MCP_PID="$!"
  exec 3>"${MCP_STDIN_FIFO}"
  exec 4<"${MCP_STDOUT_FIFO}"
  MCP_IN=3
  MCP_OUT=4
}

mcp_send_request() {
  local payload="$1"
  local out_file="$2"
  printf '%s\n' "${payload}" >&"${MCP_IN}"
  IFS= read -r line <&"${MCP_OUT}" || fail "MCP server closed before response"
  printf '%s\n' "${line}" >"${out_file}"
  jq -e '.error == null' "${out_file}" >/dev/null || fail "MCP error in ${out_file}: $(cat "${out_file}")"
}

mcp_send_notification() {
  local payload="$1"
  printf '%s\n' "${payload}" >&"${MCP_IN}"
}

mcp_call_tool() {
  local id="$1"
  local name="$2"
  local args_json="$3"
  local out_file="$4"
  local meta_json="${5:-null}"
  local params
  if [[ "${meta_json}" == "null" ]]; then
    params="$(jq -cn --arg name "${name}" --argjson args "${args_json}" '{name: $name, arguments: $args}')"
  else
    params="$(jq -cn --arg name "${name}" --argjson args "${args_json}" --argjson meta "${meta_json}" '{name: $name, arguments: $args, _meta: $meta}')"
  fi
  mcp_send_request "$(jq -cn --argjson id "${id}" --argjson params "${params}" '{jsonrpc:"2.0", id:$id, method:"tools/call", params:$params}')" "${out_file}"
}

verify_installed_mcp_context_pack() {
  local label="$1"
  local command="$2"
  shift 2
  local args=("$@")
  local in_fifo="${WORK_DIR}/${label}.mcp.stdin"
  local out_fifo="${WORK_DIR}/${label}.mcp.stdout"
  local stderr_file="${LOG_DIR}/${label}-mcp.stderr"
  local init_file="${LOG_DIR}/${label}-mcp-initialize.json"
  local tools_file="${LOG_DIR}/${label}-mcp-tools-list.json"
  local pack_file="${LOG_DIR}/${label}-mcp-context-pack.json"
  local pid

  mkfifo "${in_fifo}" "${out_fifo}"
  (
    cd "${SMALL_REPO}"
    "${command}" "${args[@]}" <"${in_fifo}" >"${out_fifo}" 2>"${stderr_file}"
  ) &
  pid="$!"
  exec 5>"${in_fifo}"
  exec 6<"${out_fifo}"

  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"triseek-installed-harness","version":"0"},"capabilities":{}}}' >&5
  IFS= read -r response <&6 || fail "${label}: MCP initialize produced no response"
  printf '%s\n' "${response}" >"${init_file}"
  assert_json '.result.serverInfo.name == "triseek"' "${init_file}"

  printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' >&5

  printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' >&5
  IFS= read -r response <&6 || fail "${label}: MCP tools/list produced no response"
  printf '%s\n' "${response}" >"${tools_file}"
  assert_json '([.result.tools[] | select(.name == "context_pack")][0].description | contains("bounded"))' "${tools_file}"

  printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"context_pack","arguments":{"goal":"fix auth panic for service accounts","intent":"bugfix","budget_tokens":1200,"max_files":4},"_meta":{"sessionId":"installed-harness-session"}}}' >&5
  IFS= read -r response <&6 || fail "${label}: MCP context_pack produced no response"
  printf '%s\n' "${response}" >"${pack_file}"
  assert_json '.result.isError == false and .result.structuredContent.version == "1" and .result.structuredContent.intent == "bugfix" and (.result.structuredContent.items | length >= 1 and length <= 4) and any(.result.structuredContent.items[]; (.path | sub("^\\./"; "")) == "src/auth.rs") and all(.result.structuredContent.items[]; has("content") | not)' "${pack_file}"

  exec 5>&-
  exec 6<&-
  kill "${pid}" >/dev/null 2>&1 || true
  wait "${pid}" >/dev/null 2>&1 || true
}

verify_json_mcp_config_context_pack() {
  local label="$1"
  local config_file="$2"
  local command
  local installed_args=()
  command="$(jq -r '.mcpServers.triseek.command' "${config_file}")"
  while IFS= read -r arg; do
    installed_args+=("${arg}")
  done < <(jq -r '.mcpServers.triseek.args[]' "${config_file}")
  verify_installed_mcp_context_pack "${label}" "${command}" "${installed_args[@]}"
}

verify_codex_mcp_config_context_pack() {
  local config_file="$1"
  local command
  command="$(awk -F' = ' '/^command = / { gsub(/"/, "", $2); print $2; exit }' "${config_file}")"
  verify_installed_mcp_context_pack "codex-installed" "${command}" mcp serve
}

run_mcp_checks() {
  mcp_start
  mcp_send_request '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"triseek-real-harness","version":"0"},"capabilities":{}}}' "${LOG_DIR}/mcp-initialize.json"
  assert_json '.result.serverInfo.name == "triseek"' "${LOG_DIR}/mcp-initialize.json"
  mcp_send_notification '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'

  mcp_send_request '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' "${LOG_DIR}/mcp-tools-list.json"
  assert_json '([.result.tools[].name] | index("find_files") and index("search_content") and index("search_path_and_content") and index("context_pack") and index("memo_check"))' "${LOG_DIR}/mcp-tools-list.json"

  mcp_call_tool 3 index_status '{}' "${LOG_DIR}/mcp-index-status.json"
  assert_json '.result.structuredContent.index_present == true' "${LOG_DIR}/mcp-index-status.json"

  mcp_call_tool 4 find_files '{"query":"auth","limit":10}' "${LOG_DIR}/mcp-find-files.json" '{"sessionId":"harness-session"}'
  assert_json '.result.structuredContent.files_with_matches >= 1 and any(.result.structuredContent.results[]; (.path | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/mcp-find-files.json"

  mcp_call_tool 5 search_content '{"query":"AuthConfig","mode":"literal","limit":10}' "${LOG_DIR}/mcp-search-content-1.json" '{"sessionId":"harness-session"}'
  assert_json '.result.structuredContent.files_with_matches >= 2 and (.result.structuredContent.cache == "miss" or .result.structuredContent.cache == "bypass")' "${LOG_DIR}/mcp-search-content-1.json"

  mcp_call_tool 30 context_pack '{"goal":"fix auth panic for service accounts","intent":"bugfix","budget_tokens":1200,"max_files":4}' "${LOG_DIR}/mcp-context-pack-bugfix.json" '{"sessionId":"harness-session"}'
  assert_json '.result.structuredContent.version == "1" and .result.structuredContent.intent == "bugfix" and (.result.structuredContent.items | length >= 1 and length <= 4) and any(.result.structuredContent.items[]; (.path | sub("^\\./"; "")) == "src/auth.rs") and all(.result.structuredContent.items[]; has("content") | not)' "${LOG_DIR}/mcp-context-pack-bugfix.json"

  mcp_call_tool 31 context_pack '{"goal":"review auth config change","intent":"review","changed_files":["src/auth.rs"],"budget_tokens":1200,"max_files":4}' "${LOG_DIR}/mcp-context-pack-review.json" '{"sessionId":"harness-session"}'
  assert_json '.result.structuredContent.version == "1" and .result.structuredContent.intent == "review" and any(.result.structuredContent.items[]; (.reasons | index("changed_file")))' "${LOG_DIR}/mcp-context-pack-review.json"

  local reuse_seen=0
  for attempt in 1 2 3 4; do
    mcp_call_tool "$((5 + attempt))" search_content '{"query":"AuthConfig","mode":"literal","limit":10}' "${LOG_DIR}/mcp-search-content-duplicate-${attempt}.json" '{"sessionId":"harness-session"}'
    if jq -e '.result.structuredContent.reuse_status == "fresh_duplicate"' "${LOG_DIR}/mcp-search-content-duplicate-${attempt}.json" >/dev/null; then
      cp "${LOG_DIR}/mcp-search-content-duplicate-${attempt}.json" "${LOG_DIR}/mcp-search-content-2.json"
      reuse_seen=1
      break
    fi
    sleep 1
  done
  [[ "${reuse_seen}" -eq 1 ]] || cp "${LOG_DIR}/mcp-search-content-duplicate-4.json" "${LOG_DIR}/mcp-search-content-2.json"
  assert_json '.result.structuredContent.reuse_status == "fresh_duplicate" and .result.structuredContent.results_omitted == true' "${LOG_DIR}/mcp-search-content-2.json"

  cat >>"${SMALL_REPO}/src/auth.rs" <<'EOF'

pub fn new_auth_config_marker() -> &'static str { "AuthConfigChanged" }
EOF
  sleep 1
  mcp_call_tool 20 search_content '{"query":"AuthConfig","mode":"literal","limit":10}' "${LOG_DIR}/mcp-search-content-after-edit.json" '{"sessionId":"harness-session"}'
  assert_json '(.result.structuredContent.reuse_status // "") != "fresh_duplicate" and any(.result.structuredContent.results[]; (.path | sub("^\\./"; "")) == "src/auth.rs")' "${LOG_DIR}/mcp-search-content-after-edit.json"

  mcp_call_tool 21 search_path_and_content '{"path_query":"*router.rs","content_query":"route_request","mode":"literal","limit":10}' "${LOG_DIR}/mcp-search-path-content.json" '{"sessionId":"harness-session"}'
  assert_json '.result.structuredContent.files_with_matches >= 1 and any(.result.structuredContent.results[]; (.path | sub("^\\./"; "")) == "src/router.rs")' "${LOG_DIR}/mcp-search-path-content.json"

  mcp_call_tool 22 reindex '{"mode":"incremental"}' "${LOG_DIR}/mcp-reindex.json"
  assert_json '.result.structuredContent.completed == true' "${LOG_DIR}/mcp-reindex.json"

  mcp_call_tool 23 memo_check "$(jq -cn --arg path "${SMALL_REPO}/src/auth.rs" '{path:$path, session_id:"harness-session"}')" "${LOG_DIR}/mcp-memo-check.json" '{"sessionId":"harness-session"}'
  assert_json '.result.structuredContent.recommendation != null' "${LOG_DIR}/mcp-memo-check.json"
}

run_install_checks() {
  log "install-checks"
  local install_home="${WORK_DIR}/install-home"
  local install_repo="${WORK_DIR}/install-repo"
  mkdir -p "${install_home}/user" "${install_home}/config" "${install_repo}"
  echo "fn main() {}" >"${install_repo}/main.rs"

  (
    cd "${install_repo}"
    HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
      "${BIN}" install claude-code --scope project >"${LOG_DIR}/install-claude.stdout" 2>"${LOG_DIR}/install-claude.stderr"
  )
  assert_file "${install_repo}/.mcp.json"
  assert_file "${install_repo}/.claude/settings.json"
  assert_grep '"triseek"' "${install_repo}/.mcp.json"
  assert_grep 'PreToolUse|PostToolUse' "${install_repo}/.claude/settings.json"
  verify_json_mcp_config_context_pack "claude-installed" "${install_repo}/.mcp.json"
  (
    cd "${install_repo}"
    HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
      "${BIN}" uninstall claude-code --scope project >"${LOG_DIR}/uninstall-claude.stdout" 2>"${LOG_DIR}/uninstall-claude.stderr"
  )
  ! grep -q '"triseek"' "${install_repo}/.mcp.json" || fail "Claude uninstall left triseek MCP entry"

  HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
    "${BIN}" install codex >"${LOG_DIR}/install-codex.stdout" 2>"${LOG_DIR}/install-codex.stderr"
  assert_file "${install_home}/user/.codex/config.toml"
  assert_file "${install_home}/user/.codex/hooks.json"
  assert_grep '\[mcp_servers\.triseek\]' "${install_home}/user/.codex/config.toml"
  assert_grep 'codex_hooks = true' "${install_home}/user/.codex/config.toml"
  assert_grep '"PreToolUse"' "${install_home}/user/.codex/hooks.json"
  assert_grep 'mcp__.*read_file' "${install_home}/user/.codex/hooks.json"
  verify_codex_mcp_config_context_pack "${install_home}/user/.codex/config.toml"
  HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
    "${BIN}" uninstall codex >"${LOG_DIR}/uninstall-codex.stdout" 2>"${LOG_DIR}/uninstall-codex.stderr"
  ! grep -q '\[mcp_servers\.triseek\]' "${install_home}/user/.codex/config.toml" || fail "Codex uninstall left triseek MCP entry"

  HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
    "${BIN}" install opencode >"${LOG_DIR}/install-opencode.stdout" 2>"${LOG_DIR}/install-opencode.stderr"
  assert_file "${install_home}/config/opencode/opencode.json"
  assert_file "${install_home}/config/opencode/plugins/triseek-memo.ts"
  assert_grep '"triseek"' "${install_home}/config/opencode/opencode.json"
  assert_grep 'tool.execute.after' "${install_home}/config/opencode/plugins/triseek-memo.ts"
  verify_json_mcp_config_context_pack "opencode-installed" "${install_home}/config/opencode/opencode.json"
  HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
    "${BIN}" uninstall opencode >"${LOG_DIR}/uninstall-opencode.stdout" 2>"${LOG_DIR}/uninstall-opencode.stderr"
  ! grep -q '"triseek"' "${install_home}/config/opencode/opencode.json" || fail "OpenCode uninstall left triseek MCP entry"

  HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
    "${BIN}" install pi >"${LOG_DIR}/install-pi.stdout" 2>"${LOG_DIR}/install-pi.stderr"
  assert_file "${install_home}/user/.pi/agent/settings.json"
  assert_file "${install_home}/user/.pi/agent/extensions/triseek-memo/index.ts"
  assert_grep '"triseek"' "${install_home}/user/.pi/agent/settings.json"
  assert_grep 'session_before_compact' "${install_home}/user/.pi/agent/extensions/triseek-memo/index.ts"
  verify_json_mcp_config_context_pack "pi-installed" "${install_home}/user/.pi/agent/settings.json"
  HOME="${install_home}/user" USERPROFILE="${install_home}/user" XDG_CONFIG_HOME="${install_home}/config" TRISEEK_HOME="${install_home}/state" \
    "${BIN}" uninstall pi >"${LOG_DIR}/uninstall-pi.stdout" 2>"${LOG_DIR}/uninstall-pi.stderr"
  ! grep -q '"triseek"' "${install_home}/user/.pi/agent/settings.json" || fail "Pi uninstall left triseek MCP entry"
}

run_large_repo_smoke() {
  if [[ -z "${TRISEEK_LARGE_REPO:-}" ]]; then
    log "large-repo-smoke skipped (TRISEEK_LARGE_REPO not set)"
    return
  fi
  [[ -d "${TRISEEK_LARGE_REPO}" ]] || fail "TRISEEK_LARGE_REPO is not a directory: ${TRISEEK_LARGE_REPO}"

  log "large-repo-smoke"
  local query="${TRISEEK_LARGE_QUERY:-fn}"
  run_logged large-build "${BIN}" build --json "${TRISEEK_LARGE_REPO}"
  run_logged large-search-index "${BIN}" search --no-daemon --json --engine index --max-results 20 "${query}" "${TRISEEK_LARGE_REPO}"
  run_logged large-search-rg "${BIN}" search --no-daemon --json --engine rg --max-results 20 "${query}" "${TRISEEK_LARGE_REPO}"
  assert_json '.summary.files_with_matches >= 1' "${LOG_DIR}/large-search-index.stdout"
  assert_json '.summary.files_with_matches >= 1' "${LOG_DIR}/large-search-rg.stdout"
}

write_summary() {
  cat >"${RESULTS_DIR}/summary.md" <<EOF
# TriSeek Real Harness

- Timestamp: ${timestamp}
- Results: ${RESULTS_DIR}
- Small repo: generated inside Docker
- Large repo: ${TRISEEK_LARGE_REPO:-not provided}
- Isolated HOME: ${HARNESS_HOME}
- Isolated TRISEEK_HOME: ${TRISEEK_HOME}

Checks completed:

- release binary build
- small-repo CLI features and options
- isolated daemon lifecycle
- MCP stdio tools and search reuse invalidation
- install/uninstall config checks for Claude Code, Codex, OpenCode, and Pi
- optional large-repo smoke when mounted
EOF
}

main() {
  create_small_repo
  build_binaries
  run_cli_checks
  start_daemon
  run_mcp_checks
  run_install_checks
  run_large_repo_smoke
  write_summary
  log "PASS (${RESULTS_DIR})"
}

main "$@"
