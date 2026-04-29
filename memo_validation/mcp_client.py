"""Minimal stdio MCP client for replaying TriSeek search tools."""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent


def _binary_candidates() -> list[Path]:
    suffix = ".exe" if os.name == "nt" else ""
    candidates = [REPO_ROOT / "target" / "debug" / f"triseek{suffix}"]
    which = shutil.which("triseek")
    if which:
        candidates.append(Path(which))
    return candidates


def resolve_triseek_binary() -> Path:
    for candidate in _binary_candidates():
        if candidate.exists():
            return candidate
    raise FileNotFoundError(
        "Could not locate a triseek binary. Build it first with `cargo build -p triseek`."
    )


class TriseekMcpClient:
    def __init__(self, repo_root: str | Path, *, binary: str | Path | None = None) -> None:
        self.repo_root = Path(repo_root).resolve()
        self.index_dir = self.repo_root / ".triseek-index"
        self.binary = Path(binary).resolve() if binary else resolve_triseek_binary()
        self._next_id = 1
        self._proc = subprocess.Popen(
            [
                str(self.binary),
                "mcp",
                "serve",
                "--repo",
                str(self.repo_root),
                "--index-dir",
                str(self.index_dir),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self._initialize()
        self.wait_until_ready()

    def _send(self, payload: dict[str, Any]) -> dict[str, Any]:
        if self._proc.stdin is None or self._proc.stdout is None:
            raise RuntimeError("MCP process stdio is unavailable")
        self._proc.stdin.write(json.dumps(payload) + "\n")
        self._proc.stdin.flush()
        line = self._proc.stdout.readline()
        if not line:
            stderr = ""
            if self._proc.stderr is not None:
                try:
                    stderr = self._proc.stderr.read()
                except Exception:
                    stderr = ""
            raise RuntimeError(f"MCP process exited without a response. stderr={stderr}")
        return json.loads(line)

    def _request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        request_id = self._next_id
        self._next_id += 1
        response = self._send(
            {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        )
        if response.get("id") != request_id:
            raise RuntimeError(f"Unexpected MCP response id: {response}")
        if "error" in response:
            raise RuntimeError(f"MCP {method} failed: {response['error']}")
        return response.get("result", {})

    def _notify(self, method: str, params: dict[str, Any]) -> None:
        if self._proc.stdin is None:
            raise RuntimeError("MCP process stdin is unavailable")
        self._proc.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": method, "params": params}) + "\n"
        )
        self._proc.stdin.flush()

    def _initialize(self) -> None:
        self._request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "clientInfo": {"name": "memo-validation", "version": "0"},
                "capabilities": {},
            },
        )
        self._notify("notifications/initialized", {})

    def call_tool(
        self,
        name: str,
        arguments: dict[str, Any],
        *,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {"name": name, "arguments": arguments}
        if session_id:
            params["_meta"] = {"sessionId": session_id}
        result = self._request("tools/call", params)
        content = result.get("content") or []
        content_text = ""
        if content and isinstance(content, list) and isinstance(content[0], dict):
            content_text = str(content[0].get("text") or "")
        return {
            "content_text": content_text,
            "structured_content": result.get("structuredContent") or {},
            "is_error": bool(result.get("isError")),
        }

    def list_tools(self) -> list[dict[str, Any]]:
        result = self._request("tools/list", {})
        tools = result.get("tools") or []
        if not isinstance(tools, list):
            raise RuntimeError(f"Unexpected tools/list response: {result}")
        return tools

    def wait_until_ready(self, timeout_secs: float = 30.0) -> None:
        deadline = time.monotonic() + timeout_secs
        while time.monotonic() < deadline:
            status = self.call_tool("index_status", {})
            structured = status["structured_content"]
            if structured.get("index_present") and structured.get("routing_hint") != "index_syncing":
                return
            time.sleep(0.1)
        raise TimeoutError(f"Timed out waiting for TriSeek index readiness in {self.repo_root}")

    def close(self) -> None:
        if self._proc.poll() is not None:
            self._close_pipes()
            return
        if self._proc.stdin is not None:
            try:
                self._proc.stdin.close()
            except Exception:
                pass
        self._proc.terminate()
        try:
            self._proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            self._proc.wait(timeout=5)
        self._close_pipes()

    def _close_pipes(self) -> None:
        for pipe in (self._proc.stdout, self._proc.stderr):
            if pipe is not None:
                try:
                    pipe.close()
                except Exception:
                    pass

    def __enter__(self) -> "TriseekMcpClient":
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()
