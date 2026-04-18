#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${ROOT}/target/release/triseek"
SERVER_BIN="${ROOT}/target/release/triseek-server"
SAFE_PATH="/usr/bin:/bin"

if [[ ! -x "${BIN}" || ! -x "${SERVER_BIN}" ]]; then
  echo "release_smoke: missing release binaries. Run:" >&2
  echo "  cargo build --release --locked --bin triseek --bin triseek-server" >&2
  exit 1
fi

TMP_HOME="$(mktemp -d)"
TMP_WORK="$(mktemp -d)"
LOG_DIR="${TMP_WORK}/logs"
mkdir -p "${LOG_DIR}"
cleanup() {
  rm -rf "${TMP_HOME}" "${TMP_WORK}"
}
trap cleanup EXIT

REPO="${TMP_WORK}/repo"
mkdir -p "${REPO}/src"
printf 'fn main() {}\n' > "${REPO}/src/main.rs"

echo "release_smoke: Claude project-scope install"
(
  cd "${REPO}"
  PATH="${SAFE_PATH}" "${BIN}" install claude-code --scope project >"${LOG_DIR}/claude-install.log"
)
test -f "${REPO}/.mcp.json"
test -f "${REPO}/.claude/settings.json"
grep -q '"triseek"' "${REPO}/.mcp.json"
grep -q 'memo-observe --event post-tool-use' "${REPO}/.claude/settings.json"
grep -q 'memo-observe --event session-start' "${REPO}/.claude/settings.json"
grep -q 'memo-observe --event pre-compact' "${REPO}/.claude/settings.json"

echo "release_smoke: Codex install fallback"
HOME="${TMP_HOME}" USERPROFILE="${TMP_HOME}" PATH="${SAFE_PATH}" "${BIN}" install codex >"${LOG_DIR}/codex-install.log"
test -f "${TMP_HOME}/.codex/config.toml"
test -f "${TMP_HOME}/.codex/hooks.json"
grep -q '\[mcp_servers.triseek\]' "${TMP_HOME}/.codex/config.toml"
grep -q 'codex_hooks = true' "${TMP_HOME}/.codex/config.toml"
grep -q '"PostToolUse"' "${TMP_HOME}/.codex/hooks.json"
grep -q 'memo-observe' "${TMP_HOME}/.codex/hooks.json"

echo "release_smoke: OpenCode install"
(
  cd "${REPO}"
  HOME="${TMP_HOME}" USERPROFILE="${TMP_HOME}" XDG_CONFIG_HOME="${TMP_HOME}/.config" PATH="${SAFE_PATH}" "${BIN}" install opencode >"${LOG_DIR}/opencode-install.log"
)
test -f "${TMP_HOME}/.config/opencode/opencode.json"
test -f "${TMP_HOME}/.config/opencode/plugins/triseek-memo.ts"
grep -q '"triseek"' "${TMP_HOME}/.config/opencode/opencode.json"
grep -q 'tool.execute.after' "${TMP_HOME}/.config/opencode/plugins/triseek-memo.ts"
grep -q 'memo-observe' "${TMP_HOME}/.config/opencode/plugins/triseek-memo.ts"

echo "release_smoke: Pi install"
HOME="${TMP_HOME}" USERPROFILE="${TMP_HOME}" PATH="${SAFE_PATH}" "${BIN}" install pi >"${LOG_DIR}/pi-install.log"
test -f "${TMP_HOME}/.pi/agent/settings.json"
test -f "${TMP_HOME}/.pi/agent/extensions/triseek-memo/index.ts"
grep -q '"triseek"' "${TMP_HOME}/.pi/agent/settings.json"
grep -q 'session_before_compact' "${TMP_HOME}/.pi/agent/extensions/triseek-memo/index.ts"
grep -q 'memo-observe' "${TMP_HOME}/.pi/agent/extensions/triseek-memo/index.ts"

echo "release_smoke: doctor"
(
  cd "${REPO}"
  HOME="${TMP_HOME}" USERPROFILE="${TMP_HOME}" PATH="${SAFE_PATH}" "${BIN}" doctor >"${LOG_DIR}/doctor.log"
)
grep -q '\[ok\] binary:' "${LOG_DIR}/doctor.log"

echo "release_smoke: PASS"
