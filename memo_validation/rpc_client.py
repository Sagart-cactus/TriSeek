"""JSON-RPC TCP client for the TriSeek Memo daemon.

Mirrors the connection logic in crates/search-cli/src/mcp/tools.rs:
  - Port read from ~/.triseek/daemon/daemon.port (or $TRISEEK_HOME/daemon/daemon.port)
  - Each call opens a new TCP connection, writes a newline-terminated JSON-RPC request,
    and reads a newline-terminated JSON response.
"""
from __future__ import annotations

import json
import os
import socket
from pathlib import Path


def _daemon_port_file() -> Path:
    """Mirrors triseek_home_dir() / daemon / daemon.port from Rust."""
    triseek_home = os.environ.get("TRISEEK_HOME")
    if triseek_home:
        return Path(triseek_home) / ".triseek" / "daemon" / "daemon.port"
    xdg_data = os.environ.get("XDG_DATA_HOME")
    if xdg_data:
        return Path(xdg_data) / ".triseek" / "daemon" / "daemon.port"
    home = Path.home()
    return home / ".triseek" / "daemon" / "daemon.port"


def _read_daemon_port() -> int:
    port_file = _daemon_port_file()
    if not port_file.exists():
        raise FileNotFoundError(
            f"TriSeek daemon port file not found at {port_file}. "
            "Start the daemon with: triseek daemon start"
        )
    port_str = port_file.read_text().strip()
    try:
        return int(port_str)
    except ValueError as e:
        raise ValueError(f"Malformed daemon port file {port_file}: {port_str!r}") from e


def _rpc_call(port: int, method: str, params: dict) -> dict:
    request = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
    with socket.create_connection(("127.0.0.1", port), timeout=5.0) as sock:
        sock.sendall((request + "\n").encode())
        response_bytes = b""
        while b"\n" not in response_bytes:
            chunk = sock.recv(4096)
            if not chunk:
                break
            response_bytes += chunk
    return json.loads(response_bytes.split(b"\n")[0])


class MemoRpcClient:
    """Stateless client for the Memo daemon RPC interface."""

    def __init__(self, port: int | None = None) -> None:
        self._port = port or _read_daemon_port()

    def _call(self, method: str, params: dict) -> dict:
        resp = _rpc_call(self._port, method, params)
        if "error" in resp:
            raise RuntimeError(f"RPC {method} failed: {resp['error']}")
        return resp.get("result", {})

    def memo_session_start(self, session_id: str, repo_root: str) -> dict:
        return self._call("memo_session_start", {"session_id": session_id, "repo_root": repo_root})

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
        params: dict = {"session_id": session_id, "repo_root": repo_root, "event": event}
        if path is not None:
            params["path"] = path
        if content_hash is not None:
            params["content_hash"] = content_hash
        if tokens is not None:
            params["tokens"] = tokens
        return self._call("memo_observe", params)

    def memo_status(self, session_id: str, repo_root: str, files: list[str]) -> dict:
        return self._call(
            "memo_status",
            {"session_id": session_id, "repo_root": repo_root, "files": files},
        )

    def memo_check(self, session_id: str, repo_root: str, path: str) -> dict:
        return self._call(
            "memo_check",
            {"session_id": session_id, "repo_root": repo_root, "path": path},
        )

    def memo_session(self, session_id: str) -> dict:
        return self._call("memo_session", {"session_id": session_id})

    def memo_session_end(self, session_id: str) -> dict:
        return self._call("memo_session_end", {"session_id": session_id})
