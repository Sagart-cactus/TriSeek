from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class ToolCall:
    tool_use_id: str
    name: str
    input: dict[str, Any]
    assistant_uuid: str | None
    result_uuid: str | None
    cwd: str | None
    timestamp: str | None
    usage: dict[str, Any]
    result_content: str = ""
    result_structured: Any = None


@dataclass
class ParsedTrace:
    trace_path: Path
    session_id: str | None
    cwd: str | None
    tool_calls: list[ToolCall] = field(default_factory=list)
    assistant_output_tokens: int = 0
    assistant_input_tokens: int = 0
    assistant_messages: int = 0
    tool_name_counts: dict[str, int] = field(default_factory=dict)
    assistant_text_blocks: list[str] = field(default_factory=list)


@dataclass
class NavigationSample:
    path: str | None
    tokens: int
    tool: str
    kind: str


@dataclass
class FileReadAggregate:
    path: str
    tokens: int = 0
    first_read_tokens: int = 0
    redundant_tokens: int = 0
    read_count: int = 0
    tools: list[str] = field(default_factory=list)
    was_useful: bool = False
