# Memo Installer — Manual Verification Checklist

Run these steps before shipping any release that modifies the OpenCode, Pi, or Codex installer.
All checks use the locally-built release binary and a running daemon.

---

## Prerequisites

```bash
cargo build --release -p triseek -p search-server
TRISEEK="$(pwd)/target/release/triseek"
TESTDIR="$(mktemp -d)"
echo "test file" > "$TESTDIR/lib.rs"
cd "$TESTDIR" && "$TRISEEK" daemon start
PORT=$(cat ~/.triseek/daemon/daemon.port)
```

---

## Codex — active mode (`memo_check`)

### A. Install

```bash
"$TRISEEK" install codex
```

Expected output includes:
- `registered with Codex via codex mcp add` (if Codex CLI found), or
- `wrote [mcp_servers.triseek] to ~/.codex/config.toml` (fallback)
- `memo hooks installed into ~/.codex/hooks.json`
- `enabled Codex feature flag codex_hooks = true`
- Note about issue #16732 and `memo_check` usage

### B. `memo_check` — unknown file (never read)

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"memo_check","params":{"session_id":"test","repo_root":"'"$TESTDIR"'","path":"'"$TESTDIR/lib.rs"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: `"recommendation":"reread","status":"unknown"`

### C. Observe a read, then `memo_check` → `skip_reread`

```bash
SID="codex-check-test"
# Simulate read
echo '{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","tool_name":"read","path":"'"$TESTDIR/lib.rs"'"}' \
  | "$TRISEEK" memo-observe --event post-tool-use --repo "$TESTDIR"

# Check → should be fresh
echo '{"jsonrpc":"2.0","id":2,"method":"memo_check","params":{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","path":"'"$TESTDIR/lib.rs"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: `"recommendation":"skip_reread","status":"fresh","tokens_at_last_read":<n>`

### D. Edit file, then `memo_check` → `reread` (large delta)

```bash
# Append many lines to force >10% token growth
yes "pub fn padding() -> u32 { 42 }" | head -20 >> "$TESTDIR/lib.rs"

echo '{"jsonrpc":"2.0","id":3,"method":"memo_check","params":{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","path":"'"$TESTDIR/lib.rs"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: `"recommendation":"reread","status":"stale","current_tokens":<n>` where `current_tokens > tokens_at_last_read`

### E. `reread_with_diff` (small delta < 10%)

```bash
SID2="codex-small-delta"
# Create a ~100-byte file
python3 -c "print('x' * 100)" > "$TESTDIR/small.rs"
echo '{"session_id":"'"$SID2"'","repo_root":"'"$TESTDIR"'","tool_name":"read","path":"'"$TESTDIR/small.rs"'"}' \
  | "$TRISEEK" memo-observe --event post-tool-use --repo "$TESTDIR"

# Append 5 chars (~5% growth)
printf "xxxxx" >> "$TESTDIR/small.rs"

echo '{"jsonrpc":"2.0","id":4,"method":"memo_check","params":{"session_id":"'"$SID2"'","repo_root":"'"$TESTDIR"'","path":"'"$TESTDIR/small.rs"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: `"recommendation":"reread_with_diff","status":"stale"`

---

## OpenCode — passive mode

### A. Install

```bash
"$TRISEEK" install opencode
```

Expected: `triseek: OpenCode plugin installed at ~/.config/opencode/plugins/triseek-memo.ts`

Inspect the generated file:

```bash
cat ~/.config/opencode/plugins/triseek-memo.ts
```

Verify:
- `const TRISEEK_BIN = '...'` is an absolute path to the triseek binary
- Event hook is `"tool.execute.after"`
- Whitelist includes `'read', 'edit', 'write', 'apply_patch'`
- Payload sends `tool_name: input.tool` and `tool_input: output?.args ?? input?.args ?? {}`
- `execFileSync` calls `memo-observe --event post-tool-use`

### B. Simulate Read → redundant Read

```bash
SID="opencode-verify"
# Read 1
echo '{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","tool_name":"read","path":"'"$TESTDIR/lib.rs"'"}' \
  | "$TRISEEK" memo-observe --event post-tool-use --repo "$TESTDIR"
# Read 2 (same file, no edit)
echo '{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","tool_name":"read","path":"'"$TESTDIR/lib.rs"'"}' \
  | "$TRISEEK" memo-observe --event post-tool-use --repo "$TESTDIR"
```

Check stats:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"memo_session","params":{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: `"redundant_reads_prevented":1,"tokens_saved":<n>`

### C. `memo_status` includes `current_tokens` when stale

```bash
# Edit the file after the reads
echo "// changed" >> "$TESTDIR/lib.rs"
echo '{"jsonrpc":"2.0","id":2,"method":"memo_status","params":{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: file entry has `"status":"stale"` and `"current_tokens":<n>` (greater than `tokens` field)

### D. Known limitation

OpenCode uses `process.pid` as the session ID. This is stable within one process but not a semantic UUID. Sessions reset on daemon restart. Acceptable for current release.

---

## Pi — passive mode

### A. Install

```bash
"$TRISEEK" install pi
```

Expected: `triseek: Pi extension installed at ~/.pi/agent/extensions/triseek-memo/index.ts`

Inspect the generated file:

```bash
cat ~/.pi/agent/extensions/triseek-memo/index.ts
```

Verify:
- `const TRISEEK_BIN = '...'` is an absolute path to the triseek binary
- Handles `session_start` → calls `memo-observe --event session-start`
- Handles `tool_result` with whitelist `['read', 'edit', 'write', 'bash']` → calls `memo-observe --event post-tool-use`
- Handles `session_before_compact` → calls `memo-observe --event pre-compact`
- `sessionId = event?.sessionId ?? String(process.pid)`

### B. Simulate session_start → Read → redundant Read

```bash
SID="pi-verify"
# session_start
echo '{"sessionId":"'"$SID"'","cwd":"'"$TESTDIR"'"}' \
  | "$TRISEEK" memo-observe --event session-start --repo "$TESTDIR"
# Read 1
echo '{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","tool_name":"read","path":"'"$TESTDIR/lib.rs"'"}' \
  | "$TRISEEK" memo-observe --event post-tool-use --repo "$TESTDIR"
# Read 2 (redundant)
echo '{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'","tool_name":"read","path":"'"$TESTDIR/lib.rs"'"}' \
  | "$TRISEEK" memo-observe --event post-tool-use --repo "$TESTDIR"
```

Check stats:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"memo_session","params":{"session_id":"'"$SID"'","repo_root":"'"$TESTDIR"'"}}' \
  | nc 127.0.0.1 $PORT
```

Expected: `"redundant_reads_prevented":1,"tokens_saved":<n>`

### C. Bash read tracking

The memo shim now classifies Bash payloads as reads when the hook payload includes parsed command metadata
or a simple shell read command such as `cat`, `sed -n`, `head`, or `tail`.

---

## Cleanup

```bash
"$TRISEEK" daemon stop
rm -rf "$TESTDIR"
```

---

## Results summary (2026-04-16)

| Check | Harness | Result |
|---|---|---|
| Daemon starts clean with new binary | — | PASS |
| Read → redundant Read → `redundant_reads_prevented=1` | OpenCode (simulated) | PASS |
| Read → redundant Read → `redundant_reads_prevented=1` | Pi (simulated) | PASS |
| `memo_check` unknown file → `reread` | Codex | PASS |
| `memo_check` fresh file → `skip_reread` | Codex | PASS |
| `memo_check` large delta → `reread` + `current_tokens` | Codex | PASS |
| `memo_check` small delta (<10%) → `reread_with_diff` | Codex | PASS |
| `memo_status` stale → `current_tokens` set | OpenCode | PASS |
| Plugin template absolute binary path | OpenCode + Pi | PASS |
| OpenCode event name `tool.execute.after` | OpenCode | PASS (in template) |
| Pi event names `session_start`, `tool_result`, `session_before_compact` | Pi | PASS (in template) |
| Pi `bash` tool read tracking via parsed command metadata | Pi | PASS |
