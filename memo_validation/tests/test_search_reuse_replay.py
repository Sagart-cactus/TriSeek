from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scope_spike.models import ParsedTrace, ToolCall

from memo_validation.search_reuse_replay import replay_search_trace


class MockMemoObserver:
    def __init__(self) -> None:
        self.events: list[tuple[str, str]] = []

    def memo_observe(self, session_id: str, repo_root: str, event: str, **_: object) -> dict:
        self.events.append((session_id, event))
        return {"observed": True}


class MockMcpClient:
    def __init__(self, responses: list[dict]) -> None:
        self._responses = list(responses)

    def call_tool(self, name: str, arguments: dict, *, session_id: str | None = None) -> dict:
        assert session_id is not None
        assert name in {"search_content", "find_files", "search_path_and_content"}
        assert self._responses, "unexpected tool call"
        return self._responses.pop(0)

    def close(self) -> None:
        pass

    def __enter__(self) -> "MockMcpClient":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()


def _search_call(result_content: str) -> ToolCall:
    return ToolCall(
        tool_use_id="search-1",
        name="mcp__triseek__search_content",
        input={"query": "route_auth", "mode": "literal", "limit": 20},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content=result_content,
        result_structured=None,
    )


def _edit_call(path: str) -> ToolCall:
    return ToolCall(
        tool_use_id="edit-1",
        name="Edit",
        input={"file_path": path, "old_string": "route_auth", "new_string": "route_auth_v2"},
        assistant_uuid=None,
        result_uuid=None,
        cwd="/repo",
        timestamp=None,
        usage={},
        result_content="updated",
        result_structured=None,
    )


def _parsed_trace(repo_root: Path, calls: list[ToolCall]) -> ParsedTrace:
    return ParsedTrace(
        trace_path=repo_root / "trace.jsonl",
        session_id="trace-session",
        cwd=str(repo_root),
        tool_calls=calls,
    )


class SearchReuseReplayTests(unittest.TestCase):
    def test_duplicate_search_reuse_saves_tokens(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir) / "repo"
            (repo_root / "src").mkdir(parents=True)
            (repo_root / "src/lib.rs").write_text("pub fn route_auth() {}\n")
            parsed = _parsed_trace(
                repo_root,
                [
                    _search_call("src/lib.rs:1 pub fn route_auth() { /* long detailed search output */ }"),
                    _search_call("src/lib.rs:1 pub fn route_auth() { /* long detailed search output */ }"),
                ],
            )
            memo = MockMemoObserver()
            responses = [
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
                {
                    "content_text": "reuse prior result from context",
                    "structured_content": {"reuse_status": "fresh_duplicate"},
                    "is_error": False,
                },
            ]

            result = replay_search_trace(
                parsed,
                memo,
                "run-a",
                mcp_client_factory=lambda _: MockMcpClient(responses),
                source_repo_root=repo_root,
            )

            self.assertEqual(result.total_search_calls, 2)
            self.assertEqual(result.duplicate_search_calls, 1)
            self.assertEqual(result.search_reuse_hits, 1)
            self.assertEqual(result.search_false_negatives, 0)
            self.assertGreater(result.search_tokens_saved, 0)
            self.assertEqual(memo.events[0][1], "session_start")

    def test_edit_to_matched_path_prevents_search_reuse(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir) / "repo"
            (repo_root / "src").mkdir(parents=True)
            (repo_root / "src/lib.rs").write_text("pub fn route_auth() {}\n")
            parsed = _parsed_trace(
                repo_root,
                [
                    _search_call("src/lib.rs:1 pub fn route_auth()"),
                    _edit_call(str(repo_root / "src/lib.rs")),
                    _search_call("src/lib.rs:1 pub fn route_auth()"),
                ],
            )
            responses = [
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth_v2()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
            ]

            result = replay_search_trace(
                parsed,
                MockMemoObserver(),
                "run-b",
                mcp_client_factory=lambda _: MockMcpClient(responses),
                source_repo_root=repo_root,
            )

            self.assertEqual(result.duplicate_search_calls, 1)
            self.assertEqual(result.search_reuse_hits, 0)
            self.assertEqual(result.search_eligible_tokens, 0)
            self.assertEqual(result.search_false_negatives, 0)

    def test_missing_reuse_on_eligible_duplicate_counts_false_negative(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir) / "repo"
            (repo_root / "src").mkdir(parents=True)
            (repo_root / "src/lib.rs").write_text("pub fn route_auth() {}\n")
            parsed = _parsed_trace(
                repo_root,
                [
                    _search_call("src/lib.rs:1 pub fn route_auth()"),
                    _search_call("src/lib.rs:1 pub fn route_auth()"),
                ],
            )
            responses = [
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
            ]

            result = replay_search_trace(
                parsed,
                MockMemoObserver(),
                "run-c",
                mcp_client_factory=lambda _: MockMcpClient(responses),
                source_repo_root=repo_root,
            )

            self.assertEqual(result.duplicate_search_calls, 1)
            self.assertEqual(result.search_reuse_hits, 0)
            self.assertEqual(result.search_false_negatives, 1)

    def test_compaction_disables_search_reuse_eligibility(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            repo_root = Path(tmpdir) / "repo"
            (repo_root / "src").mkdir(parents=True)
            (repo_root / "src/lib.rs").write_text("pub fn route_auth() {}\n")
            parsed = _parsed_trace(
                repo_root,
                [
                    _search_call("src/lib.rs:1 pub fn route_auth()"),
                    _search_call("src/lib.rs:1 pub fn route_auth()"),
                ],
            )
            memo = MockMemoObserver()
            responses = [
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
                {
                    "content_text": "src/lib.rs:1 pub fn route_auth()",
                    "structured_content": {"results": [{"path": "src/lib.rs"}]},
                    "is_error": False,
                },
            ]

            result = replay_search_trace(
                parsed,
                memo,
                "run-d",
                compact_at_turn=1,
                mcp_client_factory=lambda _: MockMcpClient(responses),
                source_repo_root=repo_root,
            )

            self.assertEqual(result.duplicate_search_calls, 1)
            self.assertEqual(result.search_eligible_tokens, 0)
            self.assertEqual(result.search_false_negatives, 0)
            self.assertEqual(memo.events[-1][1], "pre_compact")


if __name__ == "__main__":
    unittest.main()
