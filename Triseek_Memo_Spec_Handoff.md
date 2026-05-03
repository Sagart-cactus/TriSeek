# TriSeek Memo — Implementation Spec & Handoff

## Context

**What is Memo?** A session-aware file cache that lives inside the TriSeek daemon. When an AI agent reads a file, Memo records the content hash and token count. When the agent (or context compaction) causes a re-read of the same file, Memo intercepts and returns either "you already have this — skip" or a compact diff of what changed, instead of re-ingesting the full file.

**Why build this?** Across 12 traced coding sessions (6 explicit-prompt, 6 cold-start), agents spent 36–87% of their navigation tokens re-reading files they had already seen. The oracle elimination ratio averaged **97.3%** with explicit prompts and **89.0%** with cold-start prompts. Most of this waste comes from re-reads after context compaction — the agent's working memory is truncated, so it re-reads files it already processed earlier in the session.

**Key distinction from prompt caching:** API-level prompt caching (Anthropic, OpenAI) reduces the *cost* of reprocessing a cached prefix but does NOT reduce *context window usage*. Memo prevents tokens from entering the context at all — it's a capacity optimization, not a cost optimization. They are complementary.

**TriSeek implementation language:** Rust. The existing codebase is a Cargo workspace with 6 crates. Memo integrates into the existing daemon and MCP server — no new processes.

---

## Evidence Base

### Oracle Studies (Token Savings Ceiling)

| Study | Prompts | Runs | Avg Elimination Ratio | Median | Min | Max |
|-------|---------|------|-----------------------|--------|-----|-----|
| `study-20260415-rerun` | Explicit (symbol names given) | 6 | 97.3% | 99.0% | 89.9% | 99.7% |
| `captures-20260415-coldstart` | Ambiguous (no symbol names) | 6 | 89.0% | 97.5% | 45.1% | 99.7% |

### Representative Session: `serde-bugfix-lowercase-screaming-rule-aliases`

- Total navigation tokens: **3,658**
- Redundant re-read tokens: **3,191** (87.2% of navigation)
- Files read: 7, files useful: 3, files wasted: 4
- `serde_derive_internals/src/case.rs` read 3 times (1,874 redundant tokens)
- `serde_derive/src/internals/case.rs` read 3 times (1,317 redundant tokens)

### What Memo Would Have Saved

On the 2nd and 3rd reads, Memo returns a `fresh` status (content unchanged). The agent skips the re-read. Savings: **3,191 tokens** in a single session — and this was a small task. Larger sessions with more files and more compaction events compound the savings.

---

## Architecture

### Design Principles

1. **Zero adoption friction.** Agents keep using `Read`, `Edit`, `Bash cat` normally. Memo observes passively via harness hooks.
2. **Session-scoped.** Cache is bound to a session. No cross-session leakage. Sessions are ephemeral — no stale state across conversations.
3. **Conservative.** When uncertain (unknown session, missing hook data), Memo lets the read through. It never blocks a legitimate read.
4. **Daemon-native.** Memo state lives in the TriSeek daemon process alongside `RepoService`. No new processes, no new ports.

### Two-Layer Design

```
┌──────────────────────────────────────────────────────────┐
│  Layer 1: Passive Observer (Hooks)                       │
│                                                          │
│  PostToolUse(Read) ──→ record(session, path, hash, tokens)│
│  PostToolUse(Edit) ──→ mark_stale(session, path)         │
│  FileChanged        ──→ mark_stale(all_sessions, path)   │
│  SessionStart       ──→ create_session(id)               │
│  SessionEnd         ──→ drop_session(id)                 │
│  PreCompact         ──→ flag_compaction(session)         │
└──────────────────────────────────────────────────────────┘
            │
            │  HTTP POST to daemon (localhost TCP)
            ▼
┌──────────────────────────────────────────────────────────┐
│  TriSeek Daemon (search-server)                          │
│                                                          │
│  ServerState                                             │
│  ├── services: HashMap<String, Arc<RepoService>>         │
│  └── sessions: HashMap<SessionId, SessionState>  ◄─ NEW │
│       └── file_states: HashMap<PathBuf, FileState>       │
└──────────────────────────────────────────────────────────┘
            │
            │  MCP stdio (existing)
            ▼
┌──────────────────────────────────────────────────────────┐
│  Layer 2: Active Query Tool (MCP)                        │
│                                                          │
│  memo_status(files) ──→ fresh | stale(diff) | unknown    │
│  memo_session()     ──→ session stats, file list         │
└──────────────────────────────────────────────────────────┘
```

**Layer 1** runs outside the agent's awareness. Hooks (Claude Code, OpenCode, Pi) or prompt-guided calls (Codex) feed file-read events to the daemon.

**Layer 2** is an MCP tool the agent calls when it wants to check before re-reading. A prompt/skill/plugin-description instructs the agent: "Before re-reading a file you have seen before, call `memo_status` to check if it has changed."

---

## Existing Codebase Integration Points

### Cargo Workspace (`TriSeek/Cargo.toml`)

Memo does NOT require a new crate. It adds to two existing crates:

| Crate | What Memo Adds |
|-------|----------------|
| `search-server` | `SessionState`, `FileState` structs. New RPC methods: `memo_observe`, `memo_status`, `memo_session_start`, `memo_session_end`. Session idle cleanup. |
| `search-cli` | New MCP tools: `memo_status`, `memo_session`. Hook shim entry point for CLI mode. |

### Daemon State Extension (`search-server/src/main.rs`)

Current `ServerState`:
```rust
pub struct ServerState {
    daemon_dir: PathBuf,
    services: Mutex<HashMap<String, Arc<RepoService>>>,
}
```

Extended:
```rust
pub struct ServerState {
    daemon_dir: PathBuf,
    services: Mutex<HashMap<String, Arc<RepoService>>>,
    sessions: Mutex<HashMap<SessionId, SessionState>>,  // NEW
}
```

### File Watcher Integration (`search-index/src/watcher.rs`)

The existing `WatcherHandle` increments a `GenerationCounter` on file changes. Memo hooks into the same watcher events to mark files as stale:

```rust
// In watcher_loop(), after processing file change events:
if let Some(sessions) = sessions_ref.lock().ok() {
    for session in sessions.values_mut() {
        for changed_path in &changed_paths {
            session.mark_stale(changed_path);
        }
    }
}
```

### RPC Extension (`search-core/src/protocol.rs`)

Add new RPC methods alongside existing `search`, `status`, `frecency_select`, `reload`, `shutdown`:

```rust
// New method variants
"memo_observe"       => handle_memo_observe(state, params),
"memo_status"        => handle_memo_status(state, params),
"memo_session_start" => handle_memo_session_start(state, params),
"memo_session_end"   => handle_memo_session_end(state, params),
```

### MCP Tool Extension (`search-cli/src/mcp/schema.rs`)

Add two new tool definitions alongside existing 5 tools:

```rust
ToolDef {
    name: "memo_status",
    description: "Check if files have changed since last read in this session. \
                  Returns 'fresh' (unchanged, skip re-read), 'stale' (changed, \
                  includes diff), or 'unknown' (not seen this session).",
    params: json!({
        "type": "object",
        "required": ["files"],
        "properties": {
            "files": {
                "type": "array",
                "items": { "type": "string" },
                "description": "File paths to check"
            },
            "session_id": {
                "type": "string",
                "description": "Session identifier (auto-detected if omitted)"
            }
        }
    }),
}
```

---

## Data Model

### Core Types

```rust
use std::path::PathBuf;
use std::time::Instant;

/// Unique session identifier.
/// Claude Code: from SessionStart hook.
/// Codex: from _meta.session_id in MCP calls.
/// OpenCode/Pi: generated on first memo_observe, keyed by process ID.
type SessionId = String;

/// Per-session state. Lives in ServerState.sessions.
pub struct SessionState {
    pub id: SessionId,
    pub repo_root: PathBuf,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub compaction_count: u32,
    pub file_states: HashMap<PathBuf, FileState>,
}

/// Per-file state within a session.
pub struct FileState {
    /// xxh3 hash of file content when last read by the agent.
    pub content_hash: u64,

    /// Token count of the content (cl100k_base).
    pub tokens: u32,

    /// Number of times this file has been read in this session.
    pub read_count: u32,

    /// Tokens that were redundant (re-reads of unchanged content).
    pub redundant_tokens: u64,

    /// When the agent last read this file.
    pub last_read_at: Instant,

    /// When the agent last edited this file (if ever).
    pub last_edit_at: Option<Instant>,

    /// Current hash of the file on disk (updated by watcher).
    pub disk_hash: u64,

    /// True if disk_hash != content_hash (file changed since last read).
    pub stale: bool,
}
```

### State Transitions

```
                    ┌─────────┐
      first read    │ UNKNOWN │  (file not in session's file_states)
     ───────────►   └────┬────┘
                         │
                         ▼
                    ┌─────────┐
                    │  FRESH  │  content_hash == disk_hash
                    └────┬────┘
                         │
          ┌──────────────┼──────────────┐
          │              │              │
     agent edits    external edit   re-read (no-op,
     (PostToolUse    (FileChanged    return "fresh")
      for Edit)      hook/watcher)
          │              │
          ▼              ▼
     ┌─────────┐   ┌─────────┐
     │  STALE  │   │  STALE  │
     └────┬────┘   └────┬────┘
          │              │
     agent re-reads ─────┘
     (returns diff,
      updates hash)
          │
          ▼
     ┌─────────┐
     │  FRESH  │  (with new content_hash)
     └─────────┘
```

---

## Hook Integration

### Harness Compatibility Matrix

| Capability | Claude Code | Codex | OpenCode | Pi |
|---|---|---|---|---|
| Session ID in hooks | ✅ `session_id` | ✅ `_meta.session_id` | ❌ derive from PID | ✅ `session_id` |
| PostToolUse for Read | ✅ all tools | ❌ Bash only (#16732) | ✅ `tool.execute.after` | ✅ `tool_result` |
| PostToolUse for Edit | ✅ | ❌ same limitation | ✅ | ✅ |
| File change detection | ✅ `FileChanged` | ❌ | ❌ | ❌ |
| Compaction signal | ✅ `PreCompact` | ❌ | ⚠️ experimental | ❌ |
| Session start/end | ✅ both | ✅ both | ❌ | ✅ both |

### Hook Shim Architecture

Each harness gets a thin shim that translates hook events into daemon RPC calls. The shim is a single script/binary per harness.

**Claude Code** (`.claude/settings.json`):
```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": { "tool_name": "Read|Edit|Write" },
      "command": "triseek memo-observe --event post-tool-use"
    }],
    "SessionStart": [{
      "command": "triseek memo-observe --event session-start"
    }],
    "SessionEnd": [{
      "command": "triseek memo-observe --event session-end"
    }],
    "PreCompact": [{
      "command": "triseek memo-observe --event pre-compact"
    }]
  }
}
```

The hook receives JSON on stdin. `triseek memo-observe` parses it and sends an RPC to the daemon:

```rust
// In search-cli, new subcommand:
// triseek memo-observe --event <event_type>
//
// Reads hook JSON from stdin, extracts:
//   - session_id
//   - tool_name, tool_input (file path), tool_response (content)
// Sends RPC to daemon: memo_observe { session_id, event_type, path, content_hash, tokens }
```

**Codex** (workaround until hooks support MCP tools):

Codex passes `_meta.session_id` in MCP tool calls. The MCP server (`search-cli`) extracts this and calls the daemon's `memo_observe` RPC directly — no external hook needed. The `memo_status` tool becomes the primary interface.

**OpenCode** (`.opencode/plugins/memo.ts`):
```typescript
import { definePlugin } from "@opencode/plugin";

export default definePlugin({
  name: "triseek-memo",
  hooks: {
    "tool.execute.after": async (ctx) => {
      if (["read", "edit", "write"].includes(ctx.tool)) {
        await fetch(`http://127.0.0.1:${TRISEEK_PORT}/rpc`, {
          method: "POST",
          body: JSON.stringify({
            jsonrpc: "2.0", method: "memo_observe",
            params: { session_id: process.pid.toString(), path: ctx.input.path,
                      content_hash: xxh3(ctx.result), event: "read" }
          })
        });
      }
    }
  }
});
```

**Pi** (`~/.pi/extensions/triseek-memo/`): Same pattern as OpenCode but using Pi's extension API.

---

## MCP Tool Specifications

### `memo_status`

**Purpose:** Check whether files have changed since the agent last read them.

**When to call:** Before re-reading any file the agent suspects it has seen before, especially after context compaction.

**Parameters:**
```json
{
  "files": ["src/main.rs", "src/lib.rs"],
  "session_id": "optional-auto-detected"
}
```

**Response:**
```json
{
  "results": [
    {
      "path": "src/main.rs",
      "status": "fresh",
      "tokens": 1699,
      "read_count": 2,
      "message": "Unchanged since last read. Skip re-read to save 1,699 tokens."
    },
    {
      "path": "src/lib.rs",
      "status": "stale",
      "tokens_saved": 1420,
      "diff": "--- src/lib.rs (last read)\n+++ src/lib.rs (current)\n@@ -42,3 +42,5 @@\n+pub fn new_function() -> bool {\n+    true\n+}",
      "message": "Modified since last read. Diff above shows changes (3 lines added)."
    }
  ]
}
```

**Status values:**
- `fresh` — Content unchanged on disk since last read. Agent should skip re-read.
- `stale` — Content changed since last read. Response includes a unified diff. Agent reads the diff instead of the full file.
- `unknown` — File not seen this session. Agent should read normally.

### `memo_session`

**Purpose:** Session introspection. Shows what Memo knows about the current session.

**Parameters:**
```json
{
  "session_id": "optional-auto-detected"
}
```

**Response:**
```json
{
  "session_id": "753c51d6-aa98-4a5f-8159-8938d9536668",
  "created_at": "2026-04-15T10:23:00Z",
  "tracked_files": 12,
  "total_reads": 34,
  "redundant_reads_prevented": 18,
  "tokens_saved": 24680,
  "compaction_count": 3,
  "files": [
    { "path": "src/main.rs", "status": "fresh", "reads": 4, "tokens": 1699 },
    { "path": "src/lib.rs", "status": "stale", "reads": 2, "tokens": 1420 }
  ]
}
```

---

## RPC Protocol Extension

New methods added to the daemon's JSON-RPC 2.0 interface over TCP.

### `memo_observe`

Called by hook shims to record file-read/edit events.

```json
{
  "jsonrpc": "2.0",
  "method": "memo_observe",
  "id": 1,
  "params": {
    "session_id": "753c51d6",
    "repo_root": "/Users/dev/project",
    "event": "read",
    "path": "src/main.rs",
    "content_hash": 8472919234,
    "tokens": 1699
  }
}
```

Events: `read`, `edit`, `session_start`, `session_end`, `pre_compact`.

### `memo_status`

Called by the MCP tool to check file freshness.

```json
{
  "jsonrpc": "2.0",
  "method": "memo_status",
  "id": 2,
  "params": {
    "session_id": "753c51d6",
    "files": ["src/main.rs", "src/lib.rs"]
  }
}
```

### `memo_session_start` / `memo_session_end`

Lifecycle management. `session_end` drops all state for that session.

---

## Session Discrimination

The hardest problem: how does Memo know which session a request belongs to?

### Strategy by Harness

| Harness | Session ID Source | Reliability |
|---------|-------------------|-------------|
| Claude Code | `SessionStart` hook provides `session_id`. All subsequent hooks carry it. | High — native support |
| Codex | `_meta.session_id` in every MCP `tool/call` request | High — protocol-level |
| OpenCode | No built-in session ID. Generate one from `(PID, MCP-process-start-time)`. | Medium — assumes one session per MCP process |
| Pi | `session_start` event provides `session_id` | High — native support |

### Parallel Session Safety

Multiple sessions on the same repo MUST be isolated. Each gets its own `SessionState` keyed by `session_id`. The `file_states` maps are independent.

**Edge case:** Session A reads `main.rs`, then Session B edits `main.rs`. The file watcher fires, marking `main.rs` as stale in ALL sessions (including A). Session A's next `memo_status` call correctly returns `stale` with a diff. No cross-session leakage.

### Session Timeout

Sessions are ephemeral. If no `memo_observe` or `memo_status` call arrives for a session within **600 seconds** (configurable), the daemon drops that session's state. This prevents unbounded memory growth from orphaned sessions (e.g., agent crashed without firing `SessionEnd`).

---

## Git Subtree / Submodule Handling

Memo keys on **absolute path + session_id**, not git identity.

- `repo/vendor/libfoo/src/lib.rs` and `repo/src/lib.rs` are separate entries.
- Subtree pulls trigger file watcher events → files marked stale automatically.
- Submodule checkout changes are detected by inode change → watcher fires.
- If a subtree pull replaces a file with byte-identical content, the hash matches → Memo says `fresh`. This is correct (content is the same, agent doesn't need to re-read).

The TriSeek watcher already respects `.gitignore` and handles nested directory changes. No new watcher logic is needed for subtrees.

---

## Implementation Plan

### Phase 1: Core Cache (daemon-side)

**Goal:** `SessionState` and `FileState` structs, RPC methods, session lifecycle.

**Files to modify:**
- `crates/search-server/src/main.rs` — Add `sessions` to `ServerState`, implement RPC handlers
- `crates/search-core/src/protocol.rs` — Add `MemoObserveParams`, `MemoStatusParams`, `MemoStatusResponse` types

**New files:**
- `crates/search-server/src/memo.rs` — `SessionState`, `FileState`, state transition logic, diff generation

**Dependencies:**
- `xxhash-rust` (already in workspace) for content hashing
- `similar` crate for unified diff generation (add to `Cargo.toml`)

**Tests:**
- Unit: state transitions (unknown→fresh→stale→fresh)
- Unit: parallel session isolation
- Unit: session timeout cleanup
- Integration: RPC round-trip for `memo_observe` → `memo_status`

**Estimated scope:** ~500 lines of Rust.

### Phase 2: MCP Tools

**Goal:** `memo_status` and `memo_session` exposed as MCP tools.

**Files to modify:**
- `crates/search-cli/src/mcp/schema.rs` — Tool definitions
- `crates/search-cli/src/mcp/tools.rs` — Dispatch and formatting
- `crates/search-cli/src/mcp/server.rs` — Session ID extraction from `_meta`

**Tests:**
- MCP JSON-RPC round-trip with mock daemon
- Verify tool schema matches spec

**Estimated scope:** ~200 lines of Rust.

### Phase 3: Hook Shim CLI

**Goal:** `triseek memo-observe` subcommand that reads hook JSON from stdin and sends RPC.

**Files to modify:**
- `crates/search-cli/src/main.rs` — Add `memo-observe` subcommand

**New files:**
- `crates/search-cli/src/memo_shim.rs` — Stdin parsing, event mapping, RPC dispatch

**Deliverables:**
- Example `.claude/settings.json` hook config
- Example `.opencode/plugins/memo.ts` plugin
- Example `~/.pi/extensions/triseek-memo/` extension

**Estimated scope:** ~150 lines of Rust + ~50 lines per harness config.

### Phase 4: Watcher Integration

**Goal:** File changes detected by the existing watcher mark files stale across all sessions.

**Files to modify:**
- `crates/search-index/src/watcher.rs` — Pass `sessions` reference into watcher loop
- `crates/search-server/src/main.rs` — Wire `sessions` into watcher startup

**Tests:**
- Integration: edit file on disk → `memo_status` returns `stale` with correct diff

**Estimated scope:** ~50 lines of Rust.

### Phase 5: Validation

**Goal:** Replay the 12 traced sessions through Memo and measure actual token savings vs. oracle predictions.

**Build a replay harness that:**
1. Reads a Claude Code session JSONL trace
2. Simulates hook events for each Read/Edit tool call
3. Calls `memo_status` before each re-read
4. Compares: tokens consumed with Memo vs. tokens consumed without

**Success criteria:**
- Memo prevents ≥80% of redundant re-read tokens (oracle ceiling: 89–97%)
- Zero false negatives (Memo never says "fresh" when content has changed)
- Session isolation: parallel replay of two sessions produces identical results to sequential replay

---

## Diff Strategy for Stale Files

When `memo_status` returns `stale`, the response includes a unified diff. The agent reads the diff instead of the full file.

**Implementation:**
```rust
use similar::{ChangeTag, TextDiff};

fn generate_diff(old_content: &str, new_content: &str, path: &str) -> String {
    let diff = TextDiff::from_lines(old_content, new_content);
    let mut output = format!("--- {} (last read)\n+++ {} (current)\n", path, path);
    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&hunk.to_string());
    }
    output
}
```

**Problem:** Memo stores `content_hash`, not the full content. To produce a diff, it needs the original content.

**Options:**
1. **Store content in memory.** Simple but expensive for large files. Cap at 100KB per file — beyond that, return `stale` without diff and let the agent re-read.
2. **Store content on disk.** Write to `~/.triseek/memo/<session_id>/<path_hash>.snap`. Cleaned up on session end. More memory-efficient but adds I/O.
3. **Hash-only mode.** Don't produce diffs. Just return `stale` and let the agent re-read. Simplest, still saves tokens on `fresh` files (the majority case).

**Recommendation:** Start with option 3 (hash-only) for Phase 1. Add option 2 (disk snapshots) in a follow-up if diff-based savings prove meaningful. The oracle studies show most re-reads are of *unchanged* files (the `fresh` path), so hash-only captures the majority of savings.

---

## Token Counting

Memo needs to report token counts so the agent can make informed skip-or-read decisions.

**Approach:** Use a fast approximate tokenizer. The `cl100k_base` tokenizer (used by Claude, GPT-4) can be approximated as `byte_count / 3.5` with <10% error for English code. For the MCP response, report both `bytes` and `estimated_tokens`.

**Future:** If exact counts matter, add `tiktoken-rs` as an optional dependency. But for "should I re-read this 1,700-token file?" decisions, ±10% is fine.

---

## Configuration

Add to TriSeek's config (`~/.triseek/config.toml`):

```toml
[memo]
enabled = true                    # Kill switch
session_timeout_secs = 600        # Drop idle sessions after 10 min
max_sessions = 20                 # Cap concurrent sessions
max_files_per_session = 500       # Cap tracked files per session
snapshot_mode = "hash_only"       # "hash_only" | "disk" | "memory"
snapshot_max_file_bytes = 102400  # 100KB cap for disk/memory snapshots
```

---

## Prompt / Skill / Plugin Description

For agents to use `memo_status` effectively, the tool description must guide behavior. This goes in the MCP tool's `description` field:

```
Check if files have changed since you last read them in this session.
Call this BEFORE re-reading any file you have seen earlier in the conversation,
especially after context compaction. Returns one of:
- "fresh": file unchanged, skip the re-read to save tokens
- "stale": file changed, diff included in response
- "unknown": file not seen this session, read it normally

Example: you read src/main.rs 30 messages ago. Before reading it again,
call memo_status(files: ["src/main.rs"]). If fresh, you already have the
content — no need to re-read.
```

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Agent ignores `memo_status` and re-reads anyway | No harm — Memo is additive. Agent wastes tokens but nothing breaks. | Iterate on tool description. Measure adoption rate in traces. |
| Hook shim adds latency to every tool call | Agent feels slower | Hook shim is fire-and-forget (async POST, no blocking). Daemon handles in <1ms. |
| Session ID derivation fails (OpenCode) | Cache misses, no savings | Fall back to "unknown" for all files. Safe degradation. |
| Daemon OOM from too many sessions | Daemon crashes | `max_sessions` cap + session timeout + per-session file cap. |
| Race between file watcher and hook shim | `memo_status` returns `fresh` but file just changed | Watcher debounce is 200ms. Hook shim arrives after tool completion. In practice, the watcher fires first. Add a `disk_hash` recheck in `memo_status` as a safety net. |
| Codex hooks don't fire for MCP tools | Memo is blind on Codex | Use `_meta.session_id` from MCP calls + prompt-guided `memo_status` calls. Revisit when Codex issue #16732 is resolved. |

---

## Success Metrics

After deployment, measure across real sessions:

1. **Token savings rate:** `redundant_tokens_prevented / total_navigation_tokens`. Target: ≥50% (conservative; oracle ceiling is 89–97%).
2. **False freshness rate:** Times `memo_status` returned `fresh` but content had actually changed. Target: 0%.
3. **Adoption rate:** Fraction of re-reads preceded by a `memo_status` call. Target: ≥70% within 30 days.
4. **Latency overhead:** P99 latency of `memo_observe` RPC. Target: <5ms.
5. **Session throughput:** Concurrent sessions without degradation. Target: ≥10.

---

## Non-Goals (Explicit Scope Boundaries)

- **Semantic caching.** Memo does not understand *what* the agent is looking for. It caches whole-file state, not query results.
- **Cross-session persistence.** Session state is ephemeral. No "remember what I read yesterday."
- **Replacing Read.** Memo does not intercept or wrap the Read tool. It's a parallel tool the agent can call.
- **Multi-repo sessions.** Memo tracks one `repo_root` per session. If an agent works across repos, each gets its own session.
- **Content summarization.** Memo returns diffs, not summaries. Lossy compression is out of scope.
