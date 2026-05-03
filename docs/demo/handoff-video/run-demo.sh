#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
node "$ROOT/docs/demo/handoff-video/record-handoff-demo.mjs" "$@"
