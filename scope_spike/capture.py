from __future__ import annotations

import json
import subprocess
import uuid
from pathlib import Path
from typing import Iterable

from scope_spike.models import ParsedTrace, ToolCall


class ClaudeAuthError(RuntimeError):
    """Raised when the local Claude CLI is present but not actually usable."""


def _iter_jsonl(path: Path) -> Iterable[dict]:
    with path.open() as handle:
        for raw_line in handle:
            raw_line = raw_line.strip()
            if raw_line:
                yield json.loads(raw_line)


def _normalize_tool_result_content(content: object) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if isinstance(item, dict) and isinstance(item.get("text"), str):
                parts.append(item["text"])
            else:
                parts.append(json.dumps(item, sort_keys=True))
        return "\n".join(part for part in parts if part)
    if content is None:
        return ""
    return str(content)


def parse_claude_session(jsonl_path: str | Path) -> ParsedTrace:
    path = Path(jsonl_path)
    parsed = ParsedTrace(trace_path=path, session_id=None, cwd=None)
    pending: dict[str, ToolCall] = {}

    for event in _iter_jsonl(path):
        event_type = event.get("type")
        if parsed.session_id is None:
            parsed.session_id = event.get("sessionId")
        if parsed.cwd is None and event.get("cwd"):
            parsed.cwd = event.get("cwd")

        if event_type == "assistant":
            message = event.get("message") or {}
            usage = message.get("usage") or {}
            parsed.assistant_messages += 1
            parsed.assistant_output_tokens += int(usage.get("output_tokens") or 0)
            parsed.assistant_input_tokens += int(usage.get("input_tokens") or 0)
            content = message.get("content")
            if not isinstance(content, list):
                continue
            for item in content:
                if not isinstance(item, dict):
                    continue
                item_type = item.get("type")
                if item_type == "text" and item.get("text"):
                    parsed.assistant_text_blocks.append(str(item["text"]))
                    continue
                if item_type != "tool_use":
                    continue
                tool_name = str(item.get("name"))
                parsed.tool_name_counts[tool_name] = parsed.tool_name_counts.get(tool_name, 0) + 1
                tool_call = ToolCall(
                    tool_use_id=str(item.get("id")),
                    name=tool_name,
                    input=item.get("input") or {},
                    assistant_uuid=event.get("uuid"),
                    result_uuid=None,
                    cwd=event.get("cwd") or parsed.cwd,
                    timestamp=event.get("timestamp"),
                    usage=usage,
                )
                parsed.tool_calls.append(tool_call)
                pending[tool_call.tool_use_id] = tool_call
            continue

        if event_type != "user":
            continue

        message = event.get("message") or {}
        content = message.get("content")
        if not isinstance(content, list):
            continue
        for item in content:
            if not isinstance(item, dict) or item.get("type") != "tool_result":
                continue
            tool_use_id = item.get("tool_use_id")
            if tool_use_id not in pending:
                continue
            tool_call = pending[tool_use_id]
            tool_call.result_uuid = event.get("uuid")
            tool_call.result_content = _normalize_tool_result_content(item.get("content"))
            structured = event.get("toolUseResult")
            tool_call.result_structured = structured

    return parsed


def find_session_file(session_id: str, search_root: str | Path | None = None) -> Path:
    root = Path(search_root) if search_root else Path.home() / ".claude" / "projects"
    matches = list(root.rglob(f"{session_id}.jsonl"))
    if not matches:
        raise FileNotFoundError(f"Could not locate Claude session log for {session_id}")
    return matches[0]


def run_claude_capture(
    prompt: str,
    repo_path: str | Path,
    *,
    session_id: str | None = None,
    model: str | None = None,
    permission_mode: str = "bypassPermissions",
    allowed_tools: list[str] | None = None,
    extra_args: list[str] | None = None,
) -> dict[str, object]:
    repo = Path(repo_path)
    session_id = session_id or str(uuid.uuid4())

    cmd = [
        "claude",
        "-p",
        "--session-id",
        session_id,
        "--permission-mode",
        permission_mode,
    ]
    if model:
        cmd.extend(["--model", model])
    if allowed_tools:
        cmd.extend(["--allowedTools", ",".join(allowed_tools)])
    if extra_args:
        cmd.extend(extra_args)
    cmd.append(prompt)

    completed = subprocess.run(
        cmd,
        cwd=repo,
        capture_output=True,
        text=True,
        check=False,
    )
    combined_output = "\n".join(
        chunk for chunk in [completed.stdout.strip(), completed.stderr.strip()] if chunk
    )

    if "Failed to authenticate" in combined_output or "authentication_error" in combined_output:
        raise ClaudeAuthError(
            "The local `claude` CLI reported a 401 authentication failure. "
            "Re-authenticate Claude Code before attempting fresh study captures."
        )
    if completed.returncode != 0:
        raise RuntimeError(
            "Claude capture failed with a non-zero exit code.\n"
            f"Command: {' '.join(cmd)}\n"
            f"Output:\n{combined_output}"
        )

    trace_path = find_session_file(session_id)
    return {
        "session_id": session_id,
        "trace_path": str(trace_path),
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }
