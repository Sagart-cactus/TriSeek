"""Core replay engine for Memo Phase 5 validation.

Replays a ParsedTrace through a MemoRpcClient (real daemon or MockMemoClient),
measuring token savings and checking for false negatives.

Key alignment decisions (must match Rust memo_shim.rs):
  - Content hashing:  xxhash.xxh3_64(content_bytes).intdigest()
  - Token estimation: ceil(len(content_bytes) / 3.5)
  - Path resolution:  Path.resolve() to match Rust's canonicalize()

Disk-hash strategy (important):
  The tempdir is written once per file (first Read encounter) and NOT
  overwritten on subsequent partial re-reads.  memo_observe always sends the
  DISK hash (hash of the full file as written), not the slice hash returned by
  the Read tool.  This mirrors how memo_shim.rs works: memo_status calls
  read_disk_hash() on the full file, so the stored hash must match the full
  file for freshness checks to work correctly.
"""
from __future__ import annotations

import math
import re
import tempfile
import uuid
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import xxhash

from scope_spike.capture import parse_claude_session
from scope_spike.models import ParsedTrace, ToolCall

READ_COMMAND_RE = re.compile(r"\b(cat|head|tail|sed|nl|awk)\b")


def _content_bytes(call: ToolCall) -> bytes:
    """Extract the file content bytes returned by a tool call."""
    structured = call.result_structured if isinstance(call.result_structured, dict) else {}
    file_payload = structured.get("file") if isinstance(structured.get("file"), dict) else None
    if file_payload and isinstance(file_payload.get("content"), str):
        return file_payload["content"].encode("utf-8")
    if isinstance(structured.get("content"), str):
        return str(structured["content"]).encode("utf-8")
    if isinstance(structured.get("stdout"), str):
        return str(structured["stdout"]).encode("utf-8")
    content = call.result_content or ""
    return content.encode("utf-8")


def _content_hash(data: bytes) -> int:
    """xxh3_64 hash — must match Rust xxhash_rust::xxh3::xxh3_64."""
    return xxhash.xxh3_64(data).intdigest()


def _estimate_tokens(data: bytes) -> int:
    """Token estimate — must match memo_shim.rs: ceil(bytes / 3.5)."""
    return math.ceil(len(data) / 3.5)


def _extract_read_path(call: ToolCall) -> str | None:
    """Extract the file path from a Read or Bash-cat tool call."""
    if call.name == "Read":
        structured = call.result_structured if isinstance(call.result_structured, dict) else {}
        fp = (structured.get("file") or {}).get("filePath")
        if fp:
            return str(fp)
        return call.input.get("file_path")
    if call.name == "Bash":
        cmd = str(call.input.get("command") or "")
        if not READ_COMMAND_RE.search(cmd):
            return None
        # cat/head/tail — extract path, skipping flags like -n, -40, --lines=40
        # Pattern: command [flags…] <path> — path must not start with '-'
        for pattern in [
            r"\bcat\s+(?:-[^\s]*\s+)*(?P<p>[^-\s|;&][^\s|;&]*)",
            r"\bhead\s+(?:-[^\s]*\s+)*(?P<p>[^-\s|;&][^\s|;&]*)",
            r"\btail\s+(?:-[^\s]*\s+)*(?P<p>[^-\s|;&][^\s|;&]*)",
            r"\bnl\s+(?:-[^\s]*\s+)*(?P<p>[^-\s|;&][^\s|;&]*)",
            r"\bsed\s+-n\s+['\"][^'\"]+['\"]\s+(?P<p>[^-\s|;&][^\s|;&]*)",
        ]:
            m = re.search(pattern, cmd)
            if m:
                return m.group("p").strip("\"'")
    return None


def _extract_edit_path(call: ToolCall) -> str | None:
    """Extract the file path from an Edit, MultiEdit, or Write tool call."""
    path = call.input.get("file_path")
    if not path and isinstance(call.result_structured, dict):
        path = call.result_structured.get("filePath")
    return str(path) if path else None


def _apply_edit_to_disk(call: ToolCall, abs_path: Path) -> None:
    """Apply an Edit/Write/MultiEdit to the file on disk."""
    if call.name == "Write":
        content = call.input.get("content") or ""
        abs_path.parent.mkdir(parents=True, exist_ok=True)
        abs_path.write_text(str(content), encoding="utf-8")
        return
    if call.name == "Edit":
        old = call.input.get("old_string") or ""
        new = call.input.get("new_string") or ""
        if abs_path.exists():
            text = abs_path.read_text(encoding="utf-8")
            abs_path.write_text(text.replace(str(old), str(new), 1), encoding="utf-8")
        return
    if call.name == "MultiEdit":
        if abs_path.exists():
            text = abs_path.read_text(encoding="utf-8")
            for edit in call.input.get("edits") or []:
                old = edit.get("old_string") or ""
                new = edit.get("new_string") or ""
                text = text.replace(str(old), str(new), 1)
            abs_path.write_text(text, encoding="utf-8")
        return


@dataclass
class ReplayResult:
    session_id: str
    run_id: str
    total_reads: int = 0
    redundant_reads_prevented: int = 0
    tokens_saved: int = 0
    false_negatives: int = 0
    post_compact_false_negatives: int = 0
    actual_navigation_tokens: int = 0
    memo_navigation_tokens: int = 0
    reads_detail: list[dict] = field(default_factory=list)
    # Tokens from re-reads that Memo CAN intercept: same file read again via Read/Bash-cat
    # with no intervening edit.  This is the right denominator for the ≥80% success criterion
    # (the oracle's redundant_read_tokens also counts Grep/Glob→Read and after-edit re-reads).
    memo_eligible_redundant_tokens: int = 0


def _to_relative(path_str: str, cwd: str | None) -> str:
    """Convert an absolute trace path to a repo-relative path for use in tempdir.

    - Relative paths are returned as-is.
    - Absolute paths under `cwd` are made relative to `cwd`.
    - Other absolute paths: strip the leading '/' so they become relative.
    """
    p = Path(path_str)
    if not p.is_absolute():
        return path_str
    if cwd:
        try:
            return str(p.relative_to(cwd))
        except ValueError:
            pass
    # Fallback: strip leading slash
    return path_str.lstrip("/")


def replay_trace(
    parsed: ParsedTrace,
    client: Any,  # MemoRpcClient or MockMemoClient
    run_id: str,
    *,
    session_id: str | None = None,
    compact_at_turn: int | None = None,
) -> ReplayResult:
    """Replay all tool calls in the trace through the Memo client.

    Uses a tempdir as the repo_root so read_disk_hash() in the daemon
    sees the same bytes that the trace recorded.  Path.resolve() is
    used to match Rust's canonicalize() (important on macOS where
    /tmp -> /private/tmp).

    All paths are converted to repo-relative form before being sent to the
    daemon.  This allows both the MockMemoClient and the real daemon to
    join them against repo_root and find the files on disk.
    """
    session_id = session_id or str(uuid.uuid4())
    result = ReplayResult(session_id=session_id, run_id=run_id)
    cwd = parsed.cwd  # used to relativize absolute trace paths

    with tempfile.TemporaryDirectory() as tmpdir:
        repo_root = Path(tmpdir).resolve()
        repo_root_str = str(repo_root)

        client.memo_session_start(session_id, repo_root_str)

        # Track the last content_hash we sent for each (relative) path so we can
        # detect false negatives (memo says Fresh but content changed).
        last_hash: dict[str, int] = {}
        # Track which paths were edited since their last read-observe.
        # Used to compute memo_eligible_redundant_tokens: re-reads that Memo CAN
        # intercept (Read/Bash-cat without an intervening edit).
        edited_since_last_read: set[str] = set()
        # Paths seen before synthetic compaction. After compaction the previous
        # context window is considered lost, so the next read of one of these
        # paths must be treated as a fresh read.
        compacted_paths: set[str] = set()
        compaction_triggered = False

        for turn_index, call in enumerate(parsed.tool_calls, start=1):
            try:
                is_read = call.name == "Read" or (
                    call.name == "Bash"
                    and READ_COMMAND_RE.search(str(call.input.get("command") or ""))
                )
                is_edit = call.name in {"Edit", "MultiEdit", "Write"}

                if is_read:
                    raw_path = _extract_read_path(call)
                    if not raw_path:
                        continue
                    data = _content_bytes(call)
                    if not data:
                        continue
                    rel_path = _to_relative(raw_path, cwd)
                    tokens = _estimate_tokens(data)
                    result.actual_navigation_tokens += tokens

                    abs_path = repo_root / rel_path

                    # Only write to disk on FIRST encounter of a path.
                    # Subsequent (possibly partial) re-reads must not overwrite the
                    # full file, so that memo_status's read_disk_hash() keeps seeing
                    # the original content and can correctly return Fresh.
                    if not abs_path.exists():
                        abs_path.parent.mkdir(parents=True, exist_ok=True)
                        abs_path.write_bytes(data)

                    # Always use the DISK hash for memo_observe — not the slice hash.
                    # memo_status also reads the full disk file, so stored hash and
                    # disk hash must match for Fresh detection to work.
                    disk_hash = _content_hash(abs_path.read_bytes())

                    if rel_path in compacted_paths:
                        check_resp = client.memo_check(session_id, repo_root_str, rel_path)
                        recommendation = (
                            check_resp.get("recommendation")
                            if isinstance(check_resp, dict)
                            else None
                        )
                        if recommendation == "skip_reread":
                            result.post_compact_false_negatives += 1

                    # If we've seen this file since the most recent compaction,
                    # ask Memo for its verdict first.
                    if rel_path in last_hash:
                        # Count as Memo-eligible only if no edit occurred since last read.
                        if rel_path not in edited_since_last_read:
                            result.memo_eligible_redundant_tokens += tokens

                        status_resp = client.memo_status(session_id, repo_root_str, [rel_path])
                        results = (
                            status_resp.get("results", [])
                            if isinstance(status_resp, dict)
                            else status_resp
                        )
                        if results:
                            file_status = results[0]
                            status_val = (
                                file_status.get("status")
                                if isinstance(file_status, dict)
                                else file_status
                            )
                            detail = {
                                "path": rel_path,
                                "memo_status": status_val,
                                "tokens": tokens,
                                "disk_hash_changed": disk_hash != last_hash[rel_path],
                                "eligible": rel_path not in edited_since_last_read,
                            }
                            result.reads_detail.append(detail)

                            if status_val == "fresh" and disk_hash != last_hash[rel_path]:
                                # False negative: Memo said fresh but disk content changed
                                result.false_negatives += 1

                            if status_val == "fresh":
                                # Memo says skip — tokens are saved
                                result.memo_navigation_tokens += 0
                            else:
                                result.memo_navigation_tokens += tokens
                        # Re-read processed: clear the edited marker for this path
                        edited_since_last_read.discard(rel_path)
                    else:
                        result.memo_navigation_tokens += tokens

                    # Record the observe with the disk hash
                    client.memo_observe(
                        session_id,
                        repo_root_str,
                        "read",
                        path=rel_path,
                        content_hash=disk_hash,
                        tokens=tokens,
                    )
                    last_hash[rel_path] = disk_hash
                    compacted_paths.discard(rel_path)

                elif is_edit:
                    raw_path = _extract_edit_path(call)
                    if not raw_path:
                        continue
                    rel_path = _to_relative(raw_path, cwd)
                    abs_path = repo_root / rel_path
                    _apply_edit_to_disk(call, abs_path)
                    # Update last_hash so subsequent memo_status calls detect the change
                    if abs_path.exists():
                        last_hash[rel_path] = _content_hash(abs_path.read_bytes())
                    # Mark as edited so the next re-read is correctly classified as
                    # "after-edit" and excluded from memo_eligible_redundant_tokens.
                    if rel_path in last_hash:
                        edited_since_last_read.add(rel_path)
                    client.memo_observe(
                        session_id,
                        repo_root_str,
                        "edit",
                        path=rel_path,
                    )
            finally:
                if (
                    compact_at_turn is not None
                    and not compaction_triggered
                    and turn_index == compact_at_turn
                ):
                    client.memo_observe(
                        session_id,
                        repo_root_str,
                        "pre_compact",
                    )
                    compacted_paths = set(last_hash)
                    last_hash.clear()
                    edited_since_last_read.clear()
                    compaction_triggered = True

        # Collect final stats from the daemon
        final_stats = client.memo_session(session_id)
        if isinstance(final_stats, dict):
            result.total_reads = final_stats.get("total_reads", 0)
            result.redundant_reads_prevented = final_stats.get("redundant_reads_prevented", 0)
            result.tokens_saved = final_stats.get("tokens_saved", 0)

        client.memo_session_end(session_id)

    return result


def replay_trace_from_path(
    trace_path: str,
    client: Any,
    run_id: str,
    *,
    session_id: str | None = None,
    compact_at_turn: int | None = None,
) -> ReplayResult:
    """Convenience wrapper: parse a JSONL trace file then replay it."""
    parsed = parse_claude_session(trace_path)
    return replay_trace(parsed, client, run_id, session_id=session_id, compact_at_turn=compact_at_turn)


# ---------------------------------------------------------------------------
# MockMemoClient — in-process Python reimplementation for offline testing
# ---------------------------------------------------------------------------

class MockMemoClient:
    """Pure-Python MemoState reimplementation for unit tests (no daemon needed).

    Mirrors the state machine in crates/search-server/src/memo.rs.
    """

    def __init__(self) -> None:
        # session_id -> {path -> {content_hash, tokens, read_count, stale}}
        self._sessions: dict[str, dict[str, dict]] = defaultdict(dict)
        # session stats
        self._stats: dict[str, dict] = defaultdict(
            lambda: {
                "total_reads": 0,
                "redundant_reads_prevented": 0,
                "tokens_saved": 0,
                "compaction_count": 0,
            }
        )

    def memo_session_start(self, session_id: str, repo_root: str) -> dict:
        if session_id not in self._sessions:
            self._sessions[session_id] = {}
        return {"ok": True}

    def memo_observe(
        self,
        session_id: str,
        repo_root: str,
        event: str,
        *,
        path: str | None = None,
        content_hash: int | None = None,
        tokens: int | None = None,
    ) -> dict:
        sess = self._sessions[session_id]
        stats = self._stats[session_id]
        if event == "read" and path:
            existing = sess.get(path)
            if existing and not existing["stale"] and existing["content_hash"] == content_hash:
                # Redundant re-read
                existing["read_count"] += 1
                stats["redundant_reads_prevented"] += 1
                stats["tokens_saved"] += existing["tokens"]
                stats["total_reads"] += 1
            else:
                tok = tokens or (existing["tokens"] if existing else 0)
                sess[path] = {
                    "content_hash": content_hash or 0,
                    "tokens": tok,
                    "read_count": (existing["read_count"] + 1) if existing else 1,
                    "stale": False,
                }
                stats["total_reads"] += 1
        elif event == "edit" and path:
            if path in sess:
                sess[path]["stale"] = True
        elif event == "pre_compact":
            stats["compaction_count"] += 1
            sess.clear()
        return {"observed": True}

    def memo_check(self, session_id: str, repo_root: str, path: str) -> dict:
        sess = self._sessions.get(session_id, {})
        entry = sess.get(path)
        if entry is None:
            return {
                "path": path,
                "status": "unknown",
                "recommendation": "reread",
                "tokens_at_last_read": None,
                "current_tokens": None,
                "last_read_ago_seconds": None,
            }
        if entry["stale"]:
            return {
                "path": path,
                "status": "stale",
                "recommendation": "reread",
                "tokens_at_last_read": entry["tokens"],
                "current_tokens": None,
                "last_read_ago_seconds": None,
            }
        return {
            "path": path,
            "status": "fresh",
            "recommendation": "skip_reread",
            "tokens_at_last_read": entry["tokens"],
            "current_tokens": None,
            "last_read_ago_seconds": None,
        }

    def memo_status(self, session_id: str, repo_root: str, files: list[str]) -> dict:
        sess = self._sessions.get(session_id, {})
        results = []
        for f in files:
            entry = sess.get(f)
            if entry is None:
                results.append({"path": f, "status": "unknown", "tokens": None, "read_count": None})
            elif entry["stale"]:
                results.append({"path": f, "status": "stale", "tokens": entry["tokens"], "read_count": entry["read_count"]})
            else:
                results.append({"path": f, "status": "fresh", "tokens": entry["tokens"], "read_count": entry["read_count"]})
        return {"session_id": session_id, "results": results}

    def memo_session(self, session_id: str) -> dict:
        sess = self._sessions.get(session_id, {})
        stats = self._stats[session_id]
        files = [
            {
                "path": p,
                "status": "stale" if v["stale"] else "fresh",
                "reads": v["read_count"],
                "tokens": v["tokens"],
            }
            for p, v in sess.items()
        ]
        return {
            "session_id": session_id,
            "tracked_files": len(sess),
            "total_reads": stats["total_reads"],
            "redundant_reads_prevented": stats["redundant_reads_prevented"],
            "tokens_saved": stats["tokens_saved"],
            "compaction_count": stats["compaction_count"],
            "files": files,
        }

    def memo_session_end(self, session_id: str) -> dict:
        self._sessions.pop(session_id, None)
        self._stats.pop(session_id, None)
        return {"ok": True}
