#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT="${HANDOFF_VIDEO_OUT:-${ROOT}/docs/demo/handoff-video/output}"
DEMO_HOME="${TRISEEK_DEMO_HOME:-${OUT}/demo-home}"
CLAUDE_PROJECT="${DEMO_HOME}/claude-project"
CODEX_HOME="${DEMO_HOME}/.codex"

mkdir -p "${CLAUDE_PROJECT}" "${CODEX_HOME}"

printf 'Building latest TriSeek for the demo...\n'
cargo build --workspace >/dev/null

cat >"${CLAUDE_PROJECT}/.mcp.json" <<JSON
{
  "mcpServers": {
    "triseek": {
      "command": "${ROOT}/target/debug/triseek",
      "args": ["mcp", "serve", "--repo", "${ROOT}"]
    }
  }
}
JSON

cat >"${CODEX_HOME}/config.toml" <<TOML
[mcp_servers.triseek]
command = "${ROOT}/target/debug/triseek"
args = ["mcp", "serve", "--repo", "${ROOT}"]

[features]
codex_hooks = true
TOML

printf 'Claude Code MCP installed: %s\n' "${CLAUDE_PROJECT}/.mcp.json"
printf 'Codex MCP installed:       %s\n' "${CODEX_HOME}/config.toml"
printf 'Both point at:             %s\n' "${ROOT}/target/debug/triseek"
