#!/usr/bin/env python3
import json
import os
import pty
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path


if len(sys.argv) != 3:
    print("usage: drive_real_tui.py <claude|codex> <triseek|no-triseek>", file=sys.stderr)
    sys.exit(1)

client = sys.argv[1]
mode = sys.argv[2]
root = Path(__file__).resolve().parents[2]
bench_repo = root.parent / "triseek-bench" / "repos" / "torvalds_linux"
bench_index = root.parent / "triseek-bench" / "indexes" / "torvalds_linux"
triseek_bin = root / "target" / "release" / "triseek"
font_scratch = root / "docs" / "demo" / ".real-tui-tmp"
font_scratch.mkdir(parents=True, exist_ok=True)
workdir = bench_repo

prompt_text = (
    "Search this Linux repository for the symbol AEGIS_BLOCK_SIZE. "
    "Use exactly one repo-wide search action. "
    "If TriSeek MCP is available, use it and do not use bash or grep. "
    "Otherwise use bash with rg. "
    "Return exactly three lines using these labels: "
    "FIRST_MATCH: <path>:<line> "
    "MATCHING_FILES: <number> "
    "TOTAL_LINE_MATCHES: <number>."
)

ANSI_RE = re.compile(r"\x1b(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))")


def shell_print(line: str = "") -> None:
    print(line, flush=True)


def make_triseek_wrapper(tmpdir: Path) -> Path:
    wrapper = tmpdir / "triseek-wrapper.sh"
    wrapper.write_text(
        "\n".join(
            [
                "#!/bin/sh",
                f'exec "{triseek_bin}" "$@"',
                "",
            ]
        )
    )
    wrapper.chmod(0o755)
    return wrapper


def make_claude_mcp_config(tmpdir: Path, wrapper: Path | None) -> Path:
    if wrapper is None:
        payload = {"mcpServers": {}}
    else:
        payload = {
            "mcpServers": {
                "triseek": {
                    "command": str(wrapper),
                    "args": [
                        "mcp",
                        "serve",
                        "--repo",
                        str(bench_repo),
                        "--index-dir",
                        str(bench_index),
                    ],
                }
            }
        }
    config = tmpdir / "claude.mcp.json"
    config.write_text(json.dumps(payload))
    return config


def make_codex_home(tmpdir: Path, wrapper: Path | None) -> Path:
    temp_home = tmpdir / "home"
    codex_home = temp_home / ".codex"
    codex_home.mkdir(parents=True, exist_ok=True)
    shutil.copy2(Path.home() / ".codex" / "auth.json", codex_home / "auth.json")
    config_path = codex_home / "config.toml"
    config_text = (Path.home() / ".codex" / "config.toml").read_text()
    config_path.write_text(config_text)
    with config_path.open("a", encoding="utf-8") as fh:
        if f'[projects."{root}"]' not in config_text:
            fh.write(f'\n[projects."{root}"]\ntrust_level = "trusted"\n')
        if f'[projects."{bench_repo}"]' not in config_text:
            fh.write(f'\n[projects."{bench_repo}"]\ntrust_level = "trusted"\n')
        if wrapper is not None:
            fh.write(
                "\n[mcp_servers.triseek]\n"
                f'command = "{wrapper}"\n'
                'args = ["mcp", "serve", '
                f'"--repo", "{bench_repo}", '
                f'"--index-dir", "{bench_index}"]\n'
            )
    return temp_home


def run_shell_prelude(title: str) -> None:
    shell_print(f"$ {title}")
    time.sleep(0.4)


def run_tui(
    command: list[str],
    env: dict[str, str],
    startup_delay: float,
    idle_timeout: float,
) -> None:
    master_fd, slave_fd = pty.openpty()
    child = subprocess.Popen(
        command,
        cwd=workdir,
        env=env,
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        start_new_session=True,
        close_fds=True,
    )
    os.close(slave_fd)

    started = time.monotonic()
    last_output = started
    prompt_sent = False
    saw_post_prompt_output = False
    max_runtime = 90.0
    prompt_bytes = prompt_text.encode("utf-8")
    rendered_text = ""
    completion_detected_at: float | None = None

    try:
        while True:
            now = time.monotonic()
            if child.poll() is not None:
                break
            if not prompt_sent and now - started >= startup_delay:
                for byte in prompt_bytes:
                    os.write(master_fd, bytes([byte]))
                    time.sleep(0.002)
                os.write(master_fd, b"\r")
                prompt_sent = True
                last_output = time.monotonic()
            timeout = 0.1
            ready, _, _ = select.select([master_fd], [], [], timeout)
            if ready:
                data = os.read(master_fd, 65536)
                if not data:
                    break
                sys.stdout.buffer.write(data)
                sys.stdout.flush()
                last_output = time.monotonic()
                if prompt_sent:
                    saw_post_prompt_output = True
                    rendered_text += ANSI_RE.sub("", data.decode("utf-8", errors="ignore"))
                    if (
                        rendered_text.count("FIRST_MATCH:") >= 2
                        and rendered_text.count("MATCHING_FILES:") >= 2
                        and rendered_text.count("TOTAL_LINE_MATCHES:") >= 2
                        and completion_detected_at is None
                    ):
                        completion_detected_at = last_output
            now = time.monotonic()
            if completion_detected_at is not None and now - completion_detected_at >= 1.6:
                break
            if prompt_sent and saw_post_prompt_output and now - last_output >= idle_timeout:
                break
            if now - started >= max_runtime:
                break
    finally:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        except PermissionError:
            child.terminate()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            os.close(master_fd)
        except OSError:
            pass


if client not in {"claude", "codex"}:
    print(f"unknown client: {client}", file=sys.stderr)
    sys.exit(1)
if mode not in {"triseek", "no-triseek"}:
    print(f"unknown mode: {mode}", file=sys.stderr)
    sys.exit(1)

tmpdir = Path(tempfile.mkdtemp(prefix="real-tui-", dir=font_scratch))
wrapper = make_triseek_wrapper(tmpdir) if mode == "triseek" else None

env = os.environ.copy()
env.setdefault("TERM", "xterm-256color")
env.setdefault("COLORTERM", "truecolor")

try:
    if client == "claude":
        mcp_config = make_claude_mcp_config(tmpdir, wrapper)
        cmd = [
            "claude",
            "--dangerously-skip-permissions",
            "--permission-mode",
            "bypassPermissions",
            "--effort",
            "low",
            "--strict-mcp-config",
            "--mcp-config",
            str(mcp_config),
        ]
        if mode == "triseek":
            cmd += ["--disallowedTools", "Bash"]
        else:
            cmd += ["--allowedTools", "Bash"]
        title = "claude"
        startup_delay = 3.2
        idle_timeout = 18.0 if mode == "no-triseek" else 8.0
    else:
        temp_home = make_codex_home(tmpdir, wrapper)
        env["HOME"] = str(temp_home)
        cmd = [
            "codex",
            "--dangerously-bypass-approvals-and-sandbox",
            "-c",
            'model_reasoning_effort="low"',
        ]
        title = "codex"
        startup_delay = 2.8
        idle_timeout = 18.0 if mode == "no-triseek" else 8.0

    run_shell_prelude(title)
    run_tui(cmd, env, startup_delay, idle_timeout)
finally:
    shutil.rmtree(tmpdir, ignore_errors=True)
