from __future__ import annotations

import os
import re
from collections import defaultdict
from pathlib import Path

from scope_spike.models import FileReadAggregate, NavigationSample, ParsedTrace, ToolCall
from scope_spike.tokenizer import count_tokens

PATH_LINE_RE = re.compile(r"^(?P<path>.*?):(?P<line>\d+):(?P<body>.*)$")
READ_COMMAND_RE = re.compile(r"\b(cat|head|tail|sed|nl|awk)\b")
SEARCH_COMMAND_RE = re.compile(r"\b(rg|grep|git grep)\b")
LIST_COMMAND_RE = re.compile(r"\b(find|fd|ls)\b")


def _candidate_exists(path: str, repo_root: str | None) -> bool:
    candidate = Path(path)
    if not candidate.is_absolute() and repo_root:
        candidate = Path(repo_root) / candidate
    return candidate.exists()


def _normalize_path(path: str | None, repo_root: str | None) -> str | None:
    if not path:
        return None
    normalized = os.path.normpath(path.strip())
    if normalized.startswith("-"):
        return None
    if any(token in normalized for token in ("<<", ">>", "$(", "${", "*", ";")):
        return None
    if normalized in {"EOF", "PY", "JSON"}:
        return None
    if repo_root and Path(repo_root).exists() and not _candidate_exists(normalized, repo_root):
        return None
    if not repo_root:
        return normalized
    try:
        path_obj = Path(normalized).resolve()
        root_obj = Path(repo_root).resolve()
        return str(path_obj.relative_to(root_obj))
    except Exception:
        return normalized


def _path_text_tokens(path: str) -> int:
    return count_tokens(path)


def _read_content(call: ToolCall) -> str:
    structured = call.result_structured or {}
    file_payload = structured.get("file")
    if isinstance(file_payload, dict) and isinstance(file_payload.get("content"), str):
        return str(file_payload["content"])
    if isinstance(structured.get("content"), str):
        return str(structured["content"])
    if isinstance(structured.get("stdout"), str):
        return str(structured["stdout"])
    return call.result_content or ""


def _samples_from_read(call: ToolCall, repo_root: str | None) -> list[NavigationSample]:
    structured = call.result_structured or {}
    file_payload = structured.get("file") if isinstance(structured.get("file"), dict) else None
    path = None
    if file_payload and file_payload.get("filePath"):
        path = file_payload.get("filePath")
    elif call.input.get("file_path"):
        path = call.input.get("file_path")
    normalized_path = _normalize_path(str(path), repo_root) if path else None
    if not normalized_path:
        return []
    return [
        NavigationSample(
            path=normalized_path,
            tokens=count_tokens(_read_content(call)),
            tool="Read",
            kind="read",
        )
    ]


def _parse_search_lines(content: str, repo_root: str | None, tool_name: str) -> list[NavigationSample]:
    buckets: dict[str | None, int] = defaultdict(int)
    for raw_line in content.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("[Showing results"):
            continue
        if line.startswith("<system-reminder>") or line.startswith("</system-reminder>"):
            continue
        match = PATH_LINE_RE.match(line)
        if match:
            path = _normalize_path(match.group("path"), repo_root)
            buckets[path] += count_tokens(line)
            continue
        if "/" in line or line.endswith((".rs", ".py", ".md", ".txt", ".toml", ".yml", ".yaml")):
            path = _normalize_path(line, repo_root)
            buckets[path] += count_tokens(line)
            continue
        buckets[None] += count_tokens(line)
    return [
        NavigationSample(path=path, tokens=tokens, tool=tool_name, kind="search")
        for path, tokens in buckets.items()
    ]


def _samples_from_grep(call: ToolCall, repo_root: str | None) -> list[NavigationSample]:
    structured = call.result_structured or {}
    filenames = structured.get("filenames")
    if isinstance(filenames, list) and filenames:
        return [
            NavigationSample(
                path=_normalize_path(str(path), repo_root),
                tokens=_path_text_tokens(str(path)),
                tool="Grep",
                kind="search",
            )
            for path in filenames
        ]
    return _parse_search_lines(_read_content(call), repo_root, "Grep")


def _samples_from_glob(call: ToolCall, repo_root: str | None) -> list[NavigationSample]:
    structured = call.result_structured or {}
    filenames = structured.get("filenames")
    if isinstance(filenames, list) and filenames:
        return [
            NavigationSample(
                path=_normalize_path(str(path), repo_root),
                tokens=_path_text_tokens(str(path)),
                tool="Glob",
                kind="listing",
            )
            for path in filenames
        ]
    return _parse_search_lines(_read_content(call), repo_root, "Glob")


def _extract_single_read_path(command: str) -> str | None:
    patterns = [
        r"\bcat\s+(?P<path>[^\s|;&]+)",
        r"\bhead(?:\s+-[^\s]+)*\s+(?P<path>[^\s|;&]+)",
        r"\btail(?:\s+-[^\s]+)*\s+(?P<path>[^\s|;&]+)",
        r"\bnl(?:\s+-[^\s]+)*\s+(?P<path>[^\s|;&]+)",
        r"\bsed\s+-n\s+['\"][^'\"]+['\"]\s+(?P<path>[^\s|;&]+)",
    ]
    for pattern in patterns:
        match = re.search(pattern, command)
        if match:
            candidate = match.group("path").strip("\"'")
            if candidate not in {"|", ">", ">>"}:
                return candidate
    return None


def _samples_from_bash(call: ToolCall, repo_root: str | None) -> list[NavigationSample]:
    command = str(call.input.get("command") or "")
    content = _read_content(call)
    if not command or not content:
        return []

    if SEARCH_COMMAND_RE.search(command):
        return _parse_search_lines(content, repo_root, "Bash")

    if LIST_COMMAND_RE.search(command):
        return _parse_search_lines(content, repo_root, "Bash")

    if READ_COMMAND_RE.search(command):
        path = _extract_single_read_path(command)
        if path:
            normalized_path = _normalize_path(path, repo_root)
            if normalized_path:
                return [
                    NavigationSample(
                        path=normalized_path,
                        tokens=count_tokens(content),
                        tool="Bash",
                        kind="read",
                    )
                ]
    return []


def _extract_navigation_samples(call: ToolCall, repo_root: str | None) -> list[NavigationSample]:
    match call.name:
        case "Read":
            return _samples_from_read(call, repo_root)
        case "Grep":
            return _samples_from_grep(call, repo_root)
        case "Glob":
            return _samples_from_glob(call, repo_root)
        case "Bash":
            return _samples_from_bash(call, repo_root)
        case "LS":
            return _parse_search_lines(_read_content(call), repo_root, "LS")
        case _:
            return []


def _extract_edited_paths(tool_calls: list[ToolCall], repo_root: str | None) -> set[str]:
    edited_paths: set[str] = set()
    for call in tool_calls:
        if call.name not in {"Edit", "MultiEdit", "Write"}:
            continue
        path = call.input.get("file_path")
        if not path and call.result_structured:
            path = call.result_structured.get("filePath")
        normalized_path = _normalize_path(str(path), repo_root) if path else None
        if normalized_path:
            edited_paths.add(normalized_path)
    return edited_paths


def analyze_trace(parsed: ParsedTrace) -> dict[str, object]:
    repo_root = parsed.cwd
    edited_paths = _extract_edited_paths(parsed.tool_calls, repo_root)
    file_reads: dict[str, FileReadAggregate] = {}
    overhead_tokens = 0

    for call in parsed.tool_calls:
        for sample in _extract_navigation_samples(call, repo_root):
            if sample.tokens <= 0:
                continue
            if not sample.path:
                overhead_tokens += sample.tokens
                continue
            aggregate = file_reads.get(sample.path)
            if aggregate is None:
                aggregate = FileReadAggregate(path=sample.path)
                file_reads[sample.path] = aggregate
            aggregate.read_count += 1
            aggregate.tokens += sample.tokens
            if sample.tool not in aggregate.tools:
                aggregate.tools.append(sample.tool)
            if aggregate.read_count == 1:
                aggregate.first_read_tokens = sample.tokens
            else:
                aggregate.redundant_tokens += sample.tokens

    for aggregate in file_reads.values():
        aggregate.was_useful = aggregate.path in edited_paths

    file_details = []
    for aggregate in sorted(file_reads.values(), key=lambda item: (-item.tokens, item.path)):
        file_details.append(
            {
                "path": aggregate.path,
                "tokens": aggregate.tokens,
                "first_read_tokens": aggregate.first_read_tokens,
                "redundant_tokens": aggregate.redundant_tokens,
                "tool": aggregate.tools[0] if aggregate.tools else "unknown",
                "tools": aggregate.tools,
                "was_useful": aggregate.was_useful,
                "read_count": aggregate.read_count,
            }
        )

    navigation_tokens = overhead_tokens + sum(item["tokens"] for item in file_details)
    useful_read_tokens = sum(item["tokens"] for item in file_details if item["was_useful"])
    wasted_read_tokens = sum(item["tokens"] for item in file_details if not item["was_useful"])
    redundant_read_tokens = sum(item["redundant_tokens"] for item in file_details)

    return {
        "trace_path": str(parsed.trace_path),
        "session_id": parsed.session_id,
        "repo_root": repo_root,
        "assistant_input_tokens": parsed.assistant_input_tokens,
        "assistant_output_tokens": parsed.assistant_output_tokens,
        "reasoning_tokens": parsed.assistant_output_tokens,
        "total_tokens": parsed.assistant_output_tokens + navigation_tokens,
        "navigation_tokens": navigation_tokens,
        "navigation_overhead_tokens": overhead_tokens,
        "wasted_read_tokens": wasted_read_tokens,
        "redundant_read_tokens": redundant_read_tokens,
        "useful_read_tokens": useful_read_tokens,
        "files_read": len(file_details),
        "files_useful": sum(1 for item in file_details if item["was_useful"]),
        "files_wasted": sum(1 for item in file_details if not item["was_useful"]),
        "edited_files": sorted(edited_paths),
        "assistant_messages": parsed.assistant_messages,
        "tool_counts": parsed.tool_name_counts,
        "file_details": file_details,
    }
