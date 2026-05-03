#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys
import textwrap
import time

ROLE = sys.argv[1] if len(sys.argv) > 1 else "claude"
ROOT = pathlib.Path(os.environ.get("TRISEEK_DEMO_ROOT", pathlib.cwd())).resolve()
OUT = pathlib.Path(os.environ.get("HANDOFF_VIDEO_OUT", ROOT / "docs/demo/handoff-video/output")).resolve()
SESSION = os.environ.get("HANDOFF_SESSION_ID", "demo-handoff")
TRISEEK = ROOT / "target/debug/triseek"
RPC = ROOT / "docs/demo/handoff-video/session_rpc.py"
SNAPSHOT_ID_PATH = OUT / "current-snapshot-id.txt"
SNAPSHOT_JSON_PATH = OUT / "snapshot.json"
BRIEF_PATH = OUT / "brief.txt"
RESUME_PATH = OUT / "resume-AGENTS.md"
CLAUDE_NOTE = OUT / "claude-session-notes.md"
CODEX_NOTE = OUT / "codex-continuation.md"

messages: list[tuple[str, str]] = []
tools: list[str] = []
status = "ready"


def term_width() -> int:
    return max(92, min(132, shutil.get_terminal_size((112, 32)).columns))


def truncate(value: str, width: int) -> str:
    return value if len(value) <= width else value[: max(0, width - 1)] + "..."


def run(args: list[str], timeout: int = 90) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.setdefault("TERM", "xterm-256color")
    return subprocess.run(
        args,
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
    )


def tool(label: str, args: list[str], timeout: int = 90) -> str:
    global status
    status = label
    draw()
    result = run(args, timeout=timeout)
    ok = result.returncode == 0
    first_line = next((line.strip() for line in result.stdout.splitlines() if line.strip()), "ok" if ok else "no output")
    tools.append(("OK " if ok else "ERR ") + f"{label}: " + truncate(first_line, 86))
    if not ok:
        messages.append((agent_name(), f"That command failed, so I am stopping here: {first_line}"))
        draw()
        raise SystemExit(result.returncode)
    return result.stdout


def agent_name() -> str:
    return "Claude" if ROLE == "claude" else "Codex"


def title() -> str:
    return "Claude Code" if ROLE == "claude" else "Codex CLI"


def prompt_symbol() -> str:
    return "claude" if ROLE == "claude" else "codex"


def draw() -> None:
    width = term_width()
    inner = width - 4
    print("\033[2J\033[H", end="")
    print("+" + "-" * (width - 2) + "+")
    print("| " + truncate(f"{title()}  |  {ROOT}", inner).ljust(inner) + " |")
    print("| " + truncate(f"TriSeek session: {SESSION}  |  status: {status}", inner).ljust(inner) + " |")
    print("+" + "-" * (width - 2) + "+")

    visible_messages = messages[-7:]
    if not visible_messages:
        print("| " + "New session. Ask me to work, then ask me to hand off.".ljust(inner) + " |")
    for speaker, body in visible_messages:
        wrapped = textwrap.wrap(body, width=inner - len(speaker) - 4) or [""]
        print("| " + truncate(f"{speaker}: {wrapped[0]}", inner).ljust(inner) + " |")
        for line in wrapped[1:3]:
            print("| " + (" " * (len(speaker) + 2) + line).ljust(inner) + " |")

    print("+" + "-" * (width - 2) + "+")
    print("| " + "Tool activity".ljust(inner) + " |")
    for line in tools[-7:]:
        print("| " + truncate(line, inner).ljust(inner) + " |")
    for _ in range(max(0, 7 - len(tools[-7:]))):
        print("| " + "".ljust(inner) + " |")
    print("+" + "-" * (width - 2) + "+")
    print(f"{prompt_symbol()}> ", end="", flush=True)


def claude_work(user_text: str) -> None:
    global status
    messages.append(("User", user_text))
    draw()
    if "handoff" in user_text.lower():
        messages.append(("Claude", "I will package the session so Codex can resume with the goal, touched files, and search history."))
        draw()
        out = tool(
            "triseek snapshot create",
            [
                str(TRISEEK),
                "snapshot",
                "create",
                "--session",
                SESSION,
                "--source-harness",
                "claude_code",
                "--pin",
                "crates/search-core/src/protocol.rs:320:370",
                "--pin",
                "crates/search-server/src/snapshot.rs:1:160",
            ],
        )
        SNAPSHOT_JSON_PATH.write_text(out)
        snapshot_id = json.loads(out)["snapshot_id"]
        SNAPSHOT_ID_PATH.write_text(snapshot_id + "\n")
        brief = tool("triseek brief", [str(TRISEEK), "brief", snapshot_id, "--mode", "no-inference"])
        BRIEF_PATH.write_text(brief)
        messages.append(("Claude", f"Handoff ready for Codex. Snapshot: {snapshot_id}"))
        status = "handoff ready"
        draw()
        return

    messages.append(("Claude", "I will inspect the portability layer and leave a concrete trail in the TriSeek session."))
    draw()
    tool("triseek daemon start", [str(TRISEEK), "daemon", "start", "--idle-timeout", "300", "."], timeout=120)
    tool("session_open", ["python3", str(RPC), "open", SESSION, "Demo Claude to Codex handoff"])
    tool("session_record_action", ["python3", str(RPC), "record-search", SESSION, "session_snapshot_create"])
    CLAUDE_NOTE.write_text(
        "# Claude session notes\n\n"
        "- Goal: make TriSeek handoff portable between Claude and Codex.\n"
        "- Relevant files: protocol.rs, snapshot.rs, hydrate.rs, CLI resume/brief commands.\n"
        "- Next agent should verify the hydration payload before continuing.\n"
    )
    tools.append(f"OK wrote {CLAUDE_NOTE.name}: visible session notes")
    messages.append(("Claude", "I found the portability surface and saved session notes. Ask me to create the Codex handoff when ready."))
    status = "working context captured"
    draw()


def codex_work(user_text: str) -> None:
    global status
    messages.append(("User", user_text))
    draw()
    snapshot_id = SNAPSHOT_ID_PATH.read_text().strip() if SNAPSHOT_ID_PATH.exists() else ""
    if "restore" in user_text.lower() or "handoff" in user_text.lower():
        messages.append(("Codex", "I will restore Claude's snapshot before doing any new work."))
        draw()
        tool("triseek resume", [str(TRISEEK), "resume", snapshot_id, "--write-to", str(RESUME_PATH)])
        tool("triseek snapshot show", [str(TRISEEK), "snapshot", "show", snapshot_id])
        messages.append(("Codex", "The handoff is loaded. I have the session goal, relevant files, and warm context."))
        status = "handoff restored"
        draw()
        return

    messages.append(("Codex", "I will continue from the restored context and verify the payload is usable."))
    draw()
    payload = RESUME_PATH.read_text() if RESUME_PATH.exists() else ""
    ok = "TriSeek Hydration Payload" in payload
    CODEX_NOTE.write_text(
        "# Codex continuation\n\n"
        f"- Restored snapshot: {snapshot_id}\n"
        f"- Hydration payload present: {str(ok).lower()}\n"
        "- Continued from Claude context without rediscovering the repo.\n"
    )
    tools.append(f"OK verified hydration payload: {'present' if ok else 'missing'}")
    tools.append(f"OK wrote {CODEX_NOTE.name}: continued work after restore")
    messages.append(("Codex", "Continuation complete. I used Claude's handoff instead of starting cold."))
    status = "continued from context"
    draw()


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    draw()
    for raw in sys.stdin:
        text = raw.rstrip("\n")
        if not text:
            draw()
            continue
        if text.strip() in {"/quit", "/exit", "exit", "quit"}:
            messages.append(("User", text))
            messages.append((agent_name(), "Session closed."))
            status = "exiting"
            draw()
            time.sleep(0.8)
            print()
            return 0
        if ROLE == "claude":
            claude_work(text)
        else:
            codex_work(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
