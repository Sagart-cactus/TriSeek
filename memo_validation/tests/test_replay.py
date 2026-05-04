"""Unit tests for memo_validation.replay using MockMemoClient.

These tests run without a live daemon. They use either the sample JSONL
fixture from scope_spike or synthetic ParsedTrace objects.
"""
from __future__ import annotations

import math
import tempfile
from pathlib import Path

import pytest
import xxhash

from scope_spike.capture import parse_claude_session
from scope_spike.models import ParsedTrace, ToolCall
from memo_validation.replay import (
    MockMemoClient,
    ReplayResult,
    _content_hash,
    _estimate_tokens,
    _extract_edit_path,
    _extract_read_path,
    replay_trace,
)

SAMPLE_TRACE = (
    Path(__file__).parent.parent.parent
    / "scope_spike"
    / "tests"
    / "fixtures"
    / "sample_trace.jsonl"
)


# ---------------------------------------------------------------------------
# Token estimation and hash utilities
# ---------------------------------------------------------------------------

def test_estimate_tokens_matches_rust_formula():
    # ceil(bytes / 3.5) — same as memo_shim.rs estimate_tokens
    data = b"pub fn alpha() {}\n"
    expected = math.ceil(len(data) / 3.5)
    assert _estimate_tokens(data) == expected


def test_estimate_tokens_empty():
    assert _estimate_tokens(b"") == 0


def test_content_hash_is_deterministic():
    data = b"hello world"
    assert _content_hash(data) == _content_hash(data)


def test_content_hash_differs_for_different_content():
    assert _content_hash(b"fn a() {}") != _content_hash(b"fn b() {}")


def test_xxhash_python_matches_known_vector():
    # Cross-language alignment test.
    # This vector can be verified against Rust:
    #   use xxhash_rust::xxh3::xxh3_64;
    #   assert_eq!(xxh3_64(b""), 0x2d06800538d394c2);
    empty_hash = xxhash.xxh3_64(b"").intdigest()
    assert empty_hash == 0x2D06800538D394C2, (
        f"xxhash Python/Rust mismatch on empty input: got {empty_hash:#x}"
    )


# ---------------------------------------------------------------------------
# Path extraction helpers
# ---------------------------------------------------------------------------

def _make_read_call(file_path: str, content: str) -> ToolCall:
    return ToolCall(
        tool_use_id="t1",
        name="Read",
        input={"file_path": file_path},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content=content,
        result_structured={"file": {"filePath": file_path, "content": content}},
    )


def _make_bash_cat_call(file_path: str, content: str) -> ToolCall:
    return ToolCall(
        tool_use_id="t2",
        name="Bash",
        input={"command": f"cat {file_path}"},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content=content,
        result_structured=None,
    )


def _make_edit_call(file_path: str, old: str, new: str) -> ToolCall:
    return ToolCall(
        tool_use_id="t3",
        name="Edit",
        input={"file_path": file_path, "old_string": old, "new_string": new},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content="File updated.",
        result_structured=None,
    )


def test_extract_read_path_from_read_tool():
    call = _make_read_call("/repo/src/main.rs", "fn main() {}")
    assert _extract_read_path(call) == "/repo/src/main.rs"


def test_extract_read_path_from_bash_cat():
    call = _make_bash_cat_call("/repo/src/lib.rs", "pub fn lib() {}")
    assert _extract_read_path(call) == "/repo/src/lib.rs"


def test_extract_edit_path():
    call = _make_edit_call("/repo/src/main.rs", "fn main() {}", "fn main() { println!(); }")
    assert _extract_edit_path(call) == "/repo/src/main.rs"


def test_extract_read_path_non_read_bash_returns_none():
    call = ToolCall(
        tool_use_id="t",
        name="Bash",
        input={"command": "cargo test"},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content="running 3 tests",
        result_structured=None,
    )
    assert _extract_read_path(call) is None


# ---------------------------------------------------------------------------
# MockMemoClient state machine
# ---------------------------------------------------------------------------

def test_mock_client_unknown_fresh_stale_transitions():
    client = MockMemoClient()
    client.memo_session_start("s1", "/repo")

    # Unknown before any observe
    resp = client.memo_status("s1", "/repo", ["src/lib.rs"])
    assert resp["results"][0]["status"] == "unknown"

    data = b"pub fn alpha() {}\n"
    chash = _content_hash(data)
    tokens = _estimate_tokens(data)
    client.memo_observe("s1", "/repo", "read", path="src/lib.rs", content_hash=chash, tokens=tokens)

    # Fresh after first read
    resp = client.memo_status("s1", "/repo", ["src/lib.rs"])
    assert resp["results"][0]["status"] == "fresh"

    # Edit makes it stale
    client.memo_observe("s1", "/repo", "edit", path="src/lib.rs")
    resp = client.memo_status("s1", "/repo", ["src/lib.rs"])
    assert resp["results"][0]["status"] == "stale"

    # Re-read with new content restores fresh
    new_data = b"pub fn beta() {}\n"
    new_hash = _content_hash(new_data)
    client.memo_observe("s1", "/repo", "read", path="src/lib.rs", content_hash=new_hash, tokens=_estimate_tokens(new_data))
    resp = client.memo_status("s1", "/repo", ["src/lib.rs"])
    assert resp["results"][0]["status"] == "fresh"


def test_mock_client_redundant_read_counting():
    client = MockMemoClient()
    client.memo_session_start("s1", "/repo")

    data = b"fn hello() {}\n"
    chash = _content_hash(data)
    tokens = _estimate_tokens(data)

    # First read — not redundant
    client.memo_observe("s1", "/repo", "read", path="a.rs", content_hash=chash, tokens=tokens)
    stats = client.memo_session("s1")
    assert stats["redundant_reads_prevented"] == 0

    # Second read (unchanged) — redundant
    client.memo_observe("s1", "/repo", "read", path="a.rs", content_hash=chash, tokens=tokens)
    stats = client.memo_session("s1")
    assert stats["redundant_reads_prevented"] == 1
    assert stats["tokens_saved"] == tokens

    # Third read (unchanged) — redundant again
    client.memo_observe("s1", "/repo", "read", path="a.rs", content_hash=chash, tokens=tokens)
    stats = client.memo_session("s1")
    assert stats["redundant_reads_prevented"] == 2


def test_mock_client_session_isolation():
    client = MockMemoClient()
    data = b"fn shared() {}\n"
    chash = _content_hash(data)

    for sid in ["session-a", "session-b"]:
        client.memo_session_start(sid, "/repo")
        client.memo_observe(sid, "/repo", "read", path="shared.rs", content_hash=chash, tokens=10)

    # session-a edits shared.rs
    client.memo_observe("session-a", "/repo", "edit", path="shared.rs")

    # session-a should see stale
    resp_a = client.memo_status("session-a", "/repo", ["shared.rs"])
    assert resp_a["results"][0]["status"] == "stale"

    # session-b is independent — should still be fresh
    resp_b = client.memo_status("session-b", "/repo", ["shared.rs"])
    assert resp_b["results"][0]["status"] == "fresh"


def test_mock_client_pre_compact_clears_file_map_but_keeps_session_stats():
    client = MockMemoClient()
    client.memo_session_start("s1", "/repo")
    data = b"fn compacted() {}\n"
    tokens = _estimate_tokens(data)
    client.memo_observe(
        "s1",
        "/repo",
        "read",
        path="compacted.rs",
        content_hash=_content_hash(data),
        tokens=tokens,
    )

    before = client.memo_check("s1", "/repo", "compacted.rs")
    assert before["status"] == "fresh"
    assert before["recommendation"] == "skip_reread"

    client.memo_observe("s1", "/repo", "pre_compact")

    after = client.memo_check("s1", "/repo", "compacted.rs")
    assert after["status"] == "unknown"
    assert after["recommendation"] == "reread"

    stats = client.memo_session("s1")
    assert stats["compaction_count"] == 1
    assert stats["tracked_files"] == 0
    assert stats["total_reads"] == 1
    assert stats["redundant_reads_prevented"] == 0
    assert stats["tokens_saved"] == 0


def test_mock_client_session_end_clears_state():
    client = MockMemoClient()
    client.memo_session_start("s1", "/repo")
    data = b"fn foo() {}\n"
    client.memo_observe("s1", "/repo", "read", path="foo.rs", content_hash=_content_hash(data), tokens=5)
    stats = client.memo_session("s1")
    assert stats["tracked_files"] == 1

    client.memo_session_end("s1")
    # After session end, session is gone — memo_session creates a fresh one
    stats_after = client.memo_session("s1")
    assert stats_after["tracked_files"] == 0


# ---------------------------------------------------------------------------
# End-to-end replay with sample_trace.jsonl
# ---------------------------------------------------------------------------

def test_replay_sample_trace_no_false_negatives():
    """Replay the sample fixture and verify zero false negatives."""
    assert SAMPLE_TRACE.exists(), f"Sample fixture not found: {SAMPLE_TRACE}"
    parsed = parse_claude_session(SAMPLE_TRACE)
    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="sample")
    assert result.false_negatives == 0, f"False negatives detected: {result.false_negatives}"


def test_replay_sample_trace_detects_redundant_reads():
    """The sample trace reads src/core.py twice — Memo should prevent the re-read."""
    assert SAMPLE_TRACE.exists()
    parsed = parse_claude_session(SAMPLE_TRACE)
    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="sample")
    # src/core.py is read twice in the fixture (tool-read-core-1 and tool-read-core-2)
    assert result.redundant_reads_prevented >= 1, (
        f"Expected ≥1 redundant read prevented, got {result.redundant_reads_prevented}"
    )


def test_replay_tokens_saved_is_positive():
    """Replaying a trace with re-reads should report positive tokens_saved."""
    assert SAMPLE_TRACE.exists()
    parsed = parse_claude_session(SAMPLE_TRACE)
    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="sample")
    assert result.tokens_saved > 0


def test_replay_actual_navigation_tokens_positive():
    """The replay should accumulate navigation tokens from reads."""
    assert SAMPLE_TRACE.exists()
    parsed = parse_claude_session(SAMPLE_TRACE)
    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="sample")
    assert result.actual_navigation_tokens > 0


def test_replay_synthetic_read_edit_read():
    """
    Synthetic 3-step sequence:
    1. Read file A (first time)
    2. Edit file A
    3. Read file A again (after edit — not a redundant read)
    Expect: redundant_reads_prevented=0 (the second read followed an edit)
    """
    content_v1 = b"pub fn version_one() {}\n"
    content_v2 = b"pub fn version_two() {}\n"

    call_read_v1 = _make_read_call("src/foo.rs", content_v1.decode())
    call_edit = _make_edit_call(
        "src/foo.rs",
        "pub fn version_one() {}",
        "pub fn version_two() {}",
    )
    # Override result to reflect new content
    call_read_v2 = ToolCall(
        tool_use_id="t4",
        name="Read",
        input={"file_path": "src/foo.rs"},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content=content_v2.decode(),
        result_structured={"file": {"filePath": "src/foo.rs", "content": content_v2.decode()}},
    )

    parsed = ParsedTrace(trace_path=Path("synthetic"), session_id="syn-1", cwd="/repo")
    parsed.tool_calls = [call_read_v1, call_edit, call_read_v2]

    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="synthetic")

    assert result.redundant_reads_prevented == 0, (
        f"Re-read after edit should not be counted as prevented: {result.redundant_reads_prevented}"
    )
    assert result.false_negatives == 0


def test_replay_unchanged_reread_counts_as_redundant():
    """
    Sequence: read file A twice (no edit in between).
    The second read must be prevented (redundant).
    """
    content = b"pub fn stable() {}\n"
    call_read_1 = _make_read_call("src/stable.rs", content.decode())
    call_read_2 = _make_read_call("src/stable.rs", content.decode())

    parsed = ParsedTrace(trace_path=Path("synthetic"), session_id="syn-2", cwd="/repo")
    parsed.tool_calls = [call_read_1, call_read_2]

    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="synthetic")

    assert result.redundant_reads_prevented == 1
    assert result.false_negatives == 0
    assert result.tokens_saved > 0


def test_replay_pre_compact_forces_relearn_before_reread():
    content = b"pub fn stable() {}\n"
    call_read_1 = _make_read_call("src/stable.rs", content.decode())
    call_read_2 = _make_read_call("src/stable.rs", content.decode())

    parsed = ParsedTrace(trace_path=Path("synthetic"), session_id="syn-3", cwd="/repo")
    parsed.tool_calls = [call_read_1, call_read_2]

    client = MockMemoClient()
    result = replay_trace(parsed, client, run_id="synthetic", compact_at_turn=1)

    assert result.redundant_reads_prevented == 0
    assert result.tokens_saved == 0
    assert result.false_negatives == 0
    assert result.post_compact_false_negatives == 0
