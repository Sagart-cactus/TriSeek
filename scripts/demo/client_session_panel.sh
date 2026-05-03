#!/usr/bin/env python3
import json
import subprocess
import sys
import time
from pathlib import Path


if len(sys.argv) != 3:
    print("usage: client_session_panel.sh <claude|codex> <triseek|no-triseek>", file=sys.stderr)
    sys.exit(1)

client = sys.argv[1]
mode = sys.argv[2]
root = Path(__file__).resolve().parents[2]
cli_bin = root / "target" / "release" / "triseek"
repo = root.parent / "triseek-bench" / "repos" / "torvalds_linux"
index_dir = root.parent / "triseek-bench" / "indexes" / "torvalds_linux"
query = "AEGIS_BLOCK_SIZE"
limit = 100

RESET = "\033[0m"
DIM = "\033[2m"
BOLD = "\033[1m"
BLUE = "\033[38;5;39m"
GREEN = "\033[38;5;42m"
RED = "\033[38;5;203m"
YELLOW = "\033[38;5;221m"


def out(text: str = "") -> None:
    print(text, flush=True)


def pause(seconds: float) -> None:
    time.sleep(seconds)


def parse_rg_output(output: str) -> dict:
    first_path = None
    first_line = None
    matched_lines = None
    matched_files = None
    for raw in output.splitlines():
        obj = json.loads(raw)
        if obj["type"] == "match" and first_path is None:
            first_path = Path(obj["data"]["path"]["text"]).name
            parent = Path(obj["data"]["path"]["text"]).parent.name
            first_path = f"{parent}/{first_path}"
            first_line = obj["data"]["line_number"]
        elif obj["type"] == "summary":
            matched_lines = obj["data"]["stats"]["matched_lines"]
            matched_files = obj["data"]["stats"]["searches_with_match"]
    return {
        "first_path": first_path,
        "first_line": first_line,
        "matched_lines": matched_lines,
        "matched_files": matched_files,
    }


class McpSession:
    def __init__(self) -> None:
        self.proc: subprocess.Popen[str] | None = None

    def start(self) -> None:
        self.proc = subprocess.Popen(
            [
                str(cli_bin),
                "mcp",
                "serve",
                "--repo",
                str(repo),
                "--index-dir",
                str(index_dir),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        self.send(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "clientInfo": {"name": "demo-recorder", "version": "0"},
                    "capabilities": {},
                },
            }
        )
        self.recv()
        self.send(
            {
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {},
            }
        )

    def send(self, payload: dict) -> None:
        assert self.proc and self.proc.stdin
        self.proc.stdin.write(json.dumps(payload) + "\n")
        self.proc.stdin.flush()

    def recv(self) -> dict:
        assert self.proc and self.proc.stdout
        line = self.proc.stdout.readline()
        if not line:
            raise RuntimeError("mcp server closed stdout unexpectedly")
        return json.loads(line)

    def search_content(self, request_id: int) -> tuple[float, dict]:
        started = time.perf_counter()
        self.send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "tools/call",
                "params": {
                    "name": "search_content",
                    "arguments": {
                        "query": query,
                        "mode": "literal",
                        "limit": limit,
                    },
                },
            }
        )
        response = self.recv()
        elapsed = time.perf_counter() - started
        envelope = json.loads(response["result"]["content"][0]["text"])
        return elapsed, envelope

    def shutdown(self) -> None:
        if not self.proc:
            return
        try:
            self.send({"jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": None})
            self.recv()
        except Exception:
            pass
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
        except Exception:
            pass
        self.proc.terminate()
        try:
            self.proc.wait(timeout=2)
        except Exception:
            self.proc.kill()


if client == "claude":
    client_name = "Claude Code CLI"
    list_command = "claude mcp list"
elif client == "codex":
    client_name = "Codex CLI"
    list_command = "codex mcp list"
else:
    print(f"unknown client: {client}", file=sys.stderr)
    sys.exit(1)

if mode == "triseek":
    accent = GREEN
    session_name = "TriSeek MCP installed"
    mcp = McpSession()
    mcp.start()
    mcp.search_content(2)
elif mode == "no-triseek":
    accent = RED
    session_name = "No TriSeek installed"
    subprocess.run(
        [
            "rg",
            "--json",
            "--line-number",
            "--color",
            "never",
            "--no-heading",
            "--fixed-strings",
            "--max-count",
            str(limit),
            query,
            str(repo),
        ],
        stdout=subprocess.DEVNULL,
        check=True,
        text=True,
    )
    mcp = None
else:
    print(f"unknown mode: {mode}", file=sys.stderr)
    sys.exit(1)

sys.stdout.write("\033c")
sys.stdout.flush()
out(f"{accent}{BOLD}{client_name}{RESET}")
out(f"{DIM}workspace: torvalds/linux{RESET}")
out(f"{DIM}session: {session_name}{RESET}")
out()
pause(0.4)

out(f"{BOLD}$ {list_command}{RESET}")
if mode == "triseek":
    out("triseek  connected")
else:
    out("(no triseek server configured)")
out()
pause(0.6)

out(f"{BLUE}{BOLD}User Prompt{RESET}")
out(f"Find where {query} is defined in torvalds/linux.")
out("Search the full repo and return the first file + line,")
out("matching files, and total line matches.")
out("Use repo search only.")
out()
pause(0.6)

out(f"{BLUE}{BOLD}Assistant{RESET}")
if mode == "triseek":
    out("Using the connected TriSeek MCP server for the repo-wide lookup.")
    out()
    pause(0.4)
    out(f'{YELLOW}tool{RESET}  mcp__triseek__search_content(query="{query}", mode="literal", limit={limit})')
    elapsed, envelope = mcp.search_content(3)
    out(f"{YELLOW}status{RESET} {GREEN}completed{RESET} in {elapsed:.3f}s")
    out(
        f"{YELLOW}answer{RESET} "
        f"{envelope['results'][0]['path']}:{envelope['results'][0]['matches'][0]['line']}"
    )
    out(
        f"{YELLOW}stats{RESET}  "
        f"{envelope['files_with_matches']} files, {envelope['total_line_matches']} line matches"
    )
    out(f"{YELLOW}route{RESET}  {envelope['strategy']}")
else:
    out("TriSeek is unavailable in this session, falling back to raw grep.")
    out()
    pause(0.4)
    out(
        f'{YELLOW}tool{RESET}  '
        'bash -lc "rg --json --fixed-strings --max-count 100 AEGIS_BLOCK_SIZE torvalds_linux"'
    )
    started = time.perf_counter()
    rg_output = subprocess.run(
        [
            "rg",
            "--json",
            "--line-number",
            "--color",
            "never",
            "--no-heading",
            "--fixed-strings",
            "--max-count",
            str(limit),
            query,
            str(repo),
        ],
        capture_output=True,
        check=True,
        text=True,
    ).stdout
    elapsed = time.perf_counter() - started
    rg_summary = parse_rg_output(rg_output)
    out(f"{YELLOW}status{RESET} {RED}completed{RESET} in {elapsed:.3f}s")
    out(f"{YELLOW}answer{RESET} {rg_summary['first_path']}:{rg_summary['first_line']}")
    out(
        f"{YELLOW}stats{RESET}  "
        f"{rg_summary['matched_files']} files, {rg_summary['matched_lines']} line matches"
    )
    out(f"{YELLOW}route{RESET}  raw_rg_scan")

out()
out(f"{BLUE}{BOLD}Result{RESET}")
out("Task complete.")
pause(2.5)

if mcp is not None:
    mcp.shutdown()
