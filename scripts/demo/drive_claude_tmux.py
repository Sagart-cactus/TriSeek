#!/usr/bin/env python3
import json
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path


if len(sys.argv) != 2 or sys.argv[1] not in {"triseek", "no-triseek"}:
    print("usage: drive_claude_tmux.py <triseek|no-triseek>", file=sys.stderr)
    sys.exit(1)

mode = sys.argv[1]
root = Path(__file__).resolve().parents[2]
bench_repo = root.parent / "triseek-bench" / "repos" / "torvalds_linux"
bench_index = root.parent / "triseek-bench" / "indexes" / "torvalds_linux"
tmp_root = root / "docs" / "demo" / ".real-tui-tmp"
tmp_root.mkdir(parents=True, exist_ok=True)
prompt = (
    'Call triseek.search_content exactly once with query "EXPORT_SYMBOL_GPL", '
    'mode "literal", and limit 5. '
    "Then reply with exactly one line: ANSWER files=<files_with_matches> lines=<total_line_matches>."
)
label = "TriSeek MCP Installed" if mode == "triseek" else "No TriSeek MCP"
done = threading.Event()
attach_proc: subprocess.Popen[str] | None = None


def tmux(*args: str, check: bool = True, capture: bool = False) -> str:
    cmd = ["tmux", "-L", sock, *args]
    if capture:
        return subprocess.check_output(cmd, text=True)
    subprocess.run(cmd, check=check, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return ""


tmpdir = Path(tempfile.mkdtemp(prefix="claude-tmux-", dir=tmp_root))
sock = f"claude-demo-{int(time.time())}-{mode}"
config = tmpdir / "claude.mcp.json"
payload = {"mcpServers": {}}
if mode == "triseek":
    payload["mcpServers"]["triseek"] = {
        "command": str(root / "target" / "release" / "triseek"),
        "args": [
            "mcp",
            "serve",
            "--repo",
            str(bench_repo),
            "--index-dir",
            str(bench_index),
        ],
    }
config.write_text(json.dumps(payload))

claude_cmd = [
    "claude",
    "--dangerously-skip-permissions",
    "--permission-mode",
    "bypassPermissions",
    "--effort",
    "low",
    "--strict-mcp-config",
    "--mcp-config",
    str(config),
]
if mode == "triseek":
    claude_cmd += ["--tools", ""]
else:
    claude_cmd += ["--tools", "Bash"]
    prompt = (
        "Run exactly these bash commands and nothing else:\n"
        "files=$(rg -l --fixed-strings EXPORT_SYMBOL_GPL . | wc -l | tr -d ' ')\n"
        "lines=$(rg -n --fixed-strings EXPORT_SYMBOL_GPL . | wc -l | tr -d ' ')\n"
        "printf 'ANSWER files=%s lines=%s\\n' \"$files\" \"$lines\"\n"
        "Then reply with exactly that one ANSWER line and nothing else."
    )


def driver() -> None:
    global attach_proc
    time.sleep(6)
    tmux("send-keys", "-t", "demo", prompt, "Enter")
    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        pane = tmux("capture-pane", "-pt", "demo", capture=True)
        if pane.count("ANSWER ") >= 2:
            time.sleep(2.5)
            break
        time.sleep(0.5)
    if attach_proc is not None and attach_proc.poll() is None:
        attach_proc.terminate()
    done.set()


try:
    tmux("kill-server", check=False)
    tmux(
        "new-session",
        "-d",
        "-s",
        "demo",
        "-c",
        str(bench_repo),
        shlex.join(claude_cmd),
    )
    tmux("rename-window", "-t", "demo:0", label)
    tmux("set-option", "-g", "status", "on")
    tmux("set-option", "-g", "status-left", "")
    tmux("set-option", "-g", "status-right", "")
    tmux("set-option", "-g", "status-left-length", "0")
    tmux("set-option", "-g", "status-right-length", "0")
    tmux("set-option", "-g", "status-justify", "centre")
    tmux("set-option", "-g", "status-style", "bg=#111111,fg=#d1d5db")
    tmux("set-option", "-g", "window-status-format", label)
    tmux("set-option", "-g", "window-status-current-format", label)
    worker = threading.Thread(target=driver, daemon=True)
    worker.start()
    attach_proc = subprocess.Popen(
        ["tmux", "-L", sock, "attach-session", "-t", "demo"],
        stdout=sys.stdout,
        stderr=sys.stderr,
    )
    attach_proc.wait()
    done.wait(timeout=1)
finally:
    subprocess.run(["tmux", "-L", sock, "kill-server"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shutil.rmtree(tmpdir, ignore_errors=True)
