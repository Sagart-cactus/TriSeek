from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from memo_spike_v2.study import DEFAULT, simulate_trace
from scope_spike.capture import parse_claude_session


def _make_trace(path: Path) -> None:
    repeated = "".join(
        f"fn helper_{index}() {{ println!(\"line {index}: {{}}\", config.banner); }}\n"
        for index in range(24)
    )
    original = (
        "fn main() {\n"
        "    let config = load_config();\n"
        "    println!(\"{}\", config.banner);\n"
        "    println!(\"{}\", config.footer);\n"
        "}\n"
        f"{repeated}"
    )
    updated = (
        "fn main() {\n"
        "    let config = load_config();\n"
        "    println!(\"startup: {}\", config.banner);\n"
        "    println!(\"{}\", config.footer);\n"
        "    println!(\"ready\");\n"
        "}\n"
        f"{repeated}"
    )
    events = [
        {
            "type": "assistant",
            "uuid": "a1",
            "timestamp": "2026-04-01T00:00:01Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "assistant",
                "usage": {"input_tokens": 10, "output_tokens": 50},
                "content": [
                    {
                        "type": "tool_use",
                        "id": "read-1",
                        "name": "Read",
                        "input": {"file_path": "/repo/src/main.rs"},
                    }
                ],
            },
        },
        {
            "type": "user",
            "uuid": "u1",
            "timestamp": "2026-04-01T00:00:02Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "user",
                "content": [
                    {"tool_use_id": "read-1", "type": "tool_result", "content": original}
                ],
            },
            "toolUseResult": {
                "type": "text",
                "file": {"filePath": "/repo/src/main.rs", "content": original},
            },
        },
        {
            "type": "assistant",
            "uuid": "a2",
            "timestamp": "2026-04-01T00:00:03Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "assistant",
                "usage": {"input_tokens": 10, "output_tokens": 50},
                "content": [
                    {
                        "type": "tool_use",
                        "id": "read-2",
                        "name": "Read",
                        "input": {"file_path": "/repo/src/main.rs"},
                    }
                ],
            },
        },
        {
            "type": "user",
            "uuid": "u2",
            "timestamp": "2026-04-01T00:00:04Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "user",
                "content": [
                    {"tool_use_id": "read-2", "type": "tool_result", "content": original}
                ],
            },
            "toolUseResult": {
                "type": "text",
                "file": {"filePath": "/repo/src/main.rs", "content": original},
            },
        },
        {
            "type": "assistant",
            "uuid": "a3",
            "timestamp": "2026-04-01T00:00:05Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "assistant",
                "usage": {"input_tokens": 10, "output_tokens": 50},
                "content": [
                    {
                        "type": "tool_use",
                        "id": "edit-1",
                        "name": "Edit",
                        "input": {
                            "file_path": "/repo/src/main.rs",
                            "old_string": original,
                            "new_string": updated,
                        },
                    }
                ],
            },
        },
        {
            "type": "user",
            "uuid": "u3",
            "timestamp": "2026-04-01T00:00:06Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "user",
                "content": [
                    {"tool_use_id": "edit-1", "type": "tool_result", "content": "updated"}
                ],
            },
            "toolUseResult": {
                "filePath": "/repo/src/main.rs",
                "oldString": original,
                "newString": updated,
            },
        },
        {
            "type": "assistant",
            "uuid": "a4",
            "timestamp": "2026-04-01T00:00:07Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "assistant",
                "usage": {"input_tokens": 10, "output_tokens": 50},
                "content": [
                    {
                        "type": "tool_use",
                        "id": "grep-1",
                        "name": "Grep",
                        "input": {"pattern": "println"},
                    }
                ],
            },
        },
        {
            "type": "user",
            "uuid": "u4",
            "timestamp": "2026-04-01T00:00:08Z",
            "cwd": "/repo",
            "sessionId": "memo-fixture",
            "message": {
                "role": "user",
                "content": [
                    {
                        "tool_use_id": "grep-1",
                        "type": "tool_result",
                        "content": "src/main.rs:3:    println!(\"startup: {}\", config.banner);\nsrc/main.rs:5:    println!(\"ready\");",
                    }
                ],
            },
            "toolUseResult": {
                "mode": "content",
                "content": "src/main.rs:3:    println!(\"startup: {}\", config.banner);\nsrc/main.rs:5:    println!(\"ready\");",
            },
        },
    ]
    with path.open("w") as handle:
        for event in events:
            handle.write(json.dumps(event) + "\n")


class MemoSpikeV2Tests(unittest.TestCase):
    def test_simulation_saves_tokens_on_rereads_and_cached_search(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            trace_path = Path(tmpdir) / "trace.jsonl"
            _make_trace(trace_path)
            parsed = parse_claude_session(trace_path)
            result = simulate_trace(parsed, DEFAULT)
            self.assertGreater(result["saved_navigation_tokens"], 0)
            self.assertEqual(result["unchanged_rereads"], 1)
            self.assertEqual(result["cached_searches"] + result["changed_searches"], 1)


if __name__ == "__main__":
    unittest.main()
