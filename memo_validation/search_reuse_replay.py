"""Replay engine for TriSeek search-result reuse validation."""
from __future__ import annotations

import json
import shutil
import tempfile
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from scope_spike.models import ParsedTrace, ToolCall
from scope_spike.tokenizer import count_tokens

from memo_validation.mcp_client import TriseekMcpClient

SEARCH_TOOL_NAMES = {
    "mcp__triseek__find_files": "find_files",
    "mcp__triseek__search_content": "search_content",
    "mcp__triseek__search_path_and_content": "search_path_and_content",
}


@dataclass
class SearchReplayResult:
    session_id: str
    run_id: str
    total_search_calls: int = 0
    duplicate_search_calls: int = 0
    search_reuse_hits: int = 0
    search_eligible_tokens: int = 0
    search_tokens_saved: int = 0
    search_false_negatives: int = 0
    post_compact_false_negatives: int = 0
    actual_search_tokens: int = 0
    replay_search_tokens: int = 0
    search_details: list[dict[str, Any]] = field(default_factory=list)


@dataclass
class _SearchHistory:
    matched_paths: set[str]
    seen_at_turn: int
    compaction_epoch: int


def _to_relative(path_str: str, cwd: str | None) -> str:
    path = Path(path_str)
    if not path.is_absolute():
        return path_str
    if cwd:
        try:
            return str(path.relative_to(cwd))
        except ValueError:
            pass
    return path_str.lstrip("/")


def _extract_edit_path(call: ToolCall) -> str | None:
    path = call.input.get("file_path")
    if not path and isinstance(call.result_structured, dict):
        path = call.result_structured.get("filePath")
    return str(path) if path else None


def _apply_edit_to_disk(call: ToolCall, abs_path: Path) -> None:
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
    if call.name == "MultiEdit" and abs_path.exists():
        text = abs_path.read_text(encoding="utf-8")
        for edit in call.input.get("edits") or []:
            old = edit.get("old_string") or ""
            new = edit.get("new_string") or ""
            text = text.replace(str(old), str(new), 1)
        abs_path.write_text(text, encoding="utf-8")


def _is_search_tool(call: ToolCall) -> bool:
    return call.name in SEARCH_TOOL_NAMES or call.name in {"Grep", "Glob"}

def _normalized_search_call(
    call: ToolCall,
    repo_root: Path,
    cwd: str | None,
) -> tuple[str, dict[str, Any]] | None:
    if call.name in SEARCH_TOOL_NAMES:
        return SEARCH_TOOL_NAMES[call.name], dict(call.input)
    if call.name == "Grep":
        pattern = call.input.get("pattern")
        if not pattern:
            return None
        path = call.input.get("path")
        glob = call.input.get("glob")
        if glob:
            return (
                "search_path_and_content",
                {
                    "path_query": glob,
                    "content_query": pattern,
                    "mode": "regex",
                },
            )
        if isinstance(path, str):
            rel_path = _to_relative(path, cwd)
            if rel_path and rel_path not in {".", ""}:
                candidate = repo_root / rel_path
                if candidate.is_file():
                    return (
                        "search_path_and_content",
                        {
                            "path_query": rel_path,
                            "content_query": pattern,
                            "mode": "regex",
                        },
                    )
        return (
            "search_content",
            {
                "query": pattern,
                "mode": "regex",
            },
        )
    if call.name == "Glob":
        pattern = call.input.get("pattern")
        if not pattern:
            return None
        return ("find_files", {"query": str(pattern)})
    return None


def _search_key(tool_name: str, arguments: dict[str, Any]) -> str:
    return json.dumps(
        {
            "tool": tool_name,
            "arguments": arguments,
        },
        sort_keys=True,
    )


def _extract_paths_from_structured(structured_content: Any) -> set[str]:
    if not isinstance(structured_content, dict):
        return set()
    results = structured_content.get("results")
    if not isinstance(results, list):
        return set()
    paths: set[str] = set()
    for item in results:
        if isinstance(item, dict) and isinstance(item.get("path"), str):
            paths.add(item["path"])
    return paths


def replay_search_trace(
    parsed: ParsedTrace,
    memo_client: Any,
    run_id: str,
    *,
    session_id: str | None = None,
    compact_at_turn: int | None = None,
    mcp_client_factory: Callable[[Path], Any] | None = None,
    source_repo_root: str | Path | None = None,
) -> SearchReplayResult:
    session_id = session_id or str(uuid.uuid4())
    result = SearchReplayResult(session_id=session_id, run_id=run_id)

    source_root = Path(source_repo_root or parsed.cwd or "").expanduser()
    if not source_root.exists():
        raise FileNotFoundError(
            f"Search replay source repo does not exist: {source_root}. "
            "Pass --source-repo-root or ensure the trace cwd still exists."
        )

    with tempfile.TemporaryDirectory() as tmpdir:
        repo_root = Path(tmpdir) / "repo"
        shutil.copytree(source_root, repo_root, dirs_exist_ok=True)
        repo_root = repo_root.resolve()
        factory = mcp_client_factory or (lambda root: TriseekMcpClient(root))
        memo_client.memo_observe(session_id, str(repo_root), "session_start")

        changed_paths: list[tuple[int, str]] = []
        search_history: dict[str, _SearchHistory] = {}
        compaction_epoch = 0
        compaction_triggered = False

        with factory(repo_root) as mcp_client:
            for turn_index, call in enumerate(parsed.tool_calls, start=1):
                try:
                    if call.name in {"Edit", "MultiEdit", "Write"}:
                        raw_path = _extract_edit_path(call)
                        if raw_path:
                            rel_path = _to_relative(raw_path, parsed.cwd)
                            abs_path = repo_root / rel_path
                            _apply_edit_to_disk(call, abs_path)
                            changed_paths.append((turn_index, rel_path))
                        continue

                    if not _is_search_tool(call):
                        continue

                    normalized = _normalized_search_call(call, repo_root, parsed.cwd)
                    if normalized is None:
                        continue
                    tool_name, arguments = normalized
                    key = _search_key(tool_name, arguments)
                    actual_tokens = count_tokens(call.result_content)
                    result.total_search_calls += 1
                    result.actual_search_tokens += actual_tokens

                    previous = search_history.get(key)
                    oracle_reusable = False
                    if previous is not None:
                        result.duplicate_search_calls += 1
                        changed_since_previous = {
                            path
                            for changed_turn, path in changed_paths
                            if changed_turn > previous.seen_at_turn
                        }
                        oracle_reusable = (
                            previous.compaction_epoch == compaction_epoch
                            and previous.matched_paths.isdisjoint(changed_since_previous)
                        )
                        if oracle_reusable:
                            result.search_eligible_tokens += actual_tokens

                    replay = mcp_client.call_tool(
                        tool_name,
                        arguments,
                        session_id=session_id,
                    )
                    replay_tokens = count_tokens(replay["content_text"])
                    result.replay_search_tokens += replay_tokens
                    structured = replay["structured_content"]
                    reused = (
                        isinstance(structured, dict)
                        and structured.get("reuse_status") == "fresh_duplicate"
                    )
                    if oracle_reusable and reused:
                        result.search_reuse_hits += 1
                        result.search_tokens_saved += max(0, actual_tokens - replay_tokens)
                    elif oracle_reusable and not reused:
                        result.search_false_negatives += 1
                        if compaction_epoch > 0:
                            result.post_compact_false_negatives += 1

                    matched_paths = _extract_paths_from_structured(structured)
                    if reused and previous is not None:
                        matched_paths = previous.matched_paths

                    search_history[key] = _SearchHistory(
                        matched_paths=matched_paths,
                        seen_at_turn=turn_index,
                        compaction_epoch=compaction_epoch,
                    )
                    result.search_details.append(
                        {
                            "turn": turn_index,
                            "tool": tool_name,
                            "duplicate": previous is not None,
                            "oracle_reusable": oracle_reusable,
                            "reused": reused,
                            "actual_tokens": actual_tokens,
                            "replay_tokens": replay_tokens,
                            "matched_paths": sorted(matched_paths),
                        }
                    )
                finally:
                    if (
                        compact_at_turn is not None
                        and not compaction_triggered
                        and turn_index == compact_at_turn
                    ):
                        memo_client.memo_observe(session_id, str(repo_root), "pre_compact")
                        compaction_epoch += 1
                        compaction_triggered = True

    return result
