#!/usr/bin/env python3
"""Small JSON-RPC helper for the TriSeek portability demo."""

from __future__ import annotations

import json
import os
import pathlib
import socket
import sys


def daemon_port() -> int:
    home = pathlib.Path(os.environ.get("TRISEEK_HOME", pathlib.Path.home() / ".triseek"))
    port_path = home / "daemon" / "daemon.port"
    return int(port_path.read_text().strip())


def rpc(method: str, params: dict) -> dict:
    with socket.create_connection(("127.0.0.1", daemon_port()), timeout=5) as sock:
        request = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        sock.sendall((json.dumps(request) + "\n").encode("utf-8"))
        line = sock.makefile().readline()
    response = json.loads(line)
    if response.get("error"):
        raise SystemExit(response["error"])
    return response.get("result") or {}


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: session_rpc.py <open|record-search|status> <session_id> [goal_or_query]", file=sys.stderr)
        return 2

    command = sys.argv[1]
    session_id = sys.argv[2]
    repo = str(pathlib.Path.cwd().resolve())

    if command == "open":
        goal = sys.argv[3] if len(sys.argv) > 3 else "Demo Claude to Codex handoff"
        result = rpc(
            "session_open",
            {"target_root": repo, "session_id": session_id, "goal": goal},
        )
        print(json.dumps(result, indent=2))
        return 0

    if command == "record-search":
        query = sys.argv[3] if len(sys.argv) > 3 else "session_snapshot_create"
        result = rpc(
            "session_record_action",
            {
                "target_root": repo,
                "session_id": session_id,
                "kind": "search",
                "payload": {
                    "query": query,
                    "kind": "literal",
                    "result_paths": [
                        "crates/search-core/src/protocol.rs",
                        "crates/search-server/src/snapshot.rs",
                    ],
                },
            },
        )
        print(json.dumps(result, indent=2))
        return 0

    if command == "status":
        result = rpc(
            "session_status",
            {"target_root": repo, "session_id": session_id},
        )
        print(json.dumps(result, indent=2))
        return 0

    print(f"unknown command: {command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
