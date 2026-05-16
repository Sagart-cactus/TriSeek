from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Callable

from context_pack_validation.fixtures import build_fixture
from context_pack_validation.report import write_reports

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RESULTS_ROOT = REPO_ROOT / "context_pack_validation" / "results"
REQUIRED_TOOLS = [
    "mcp__triseek__context_pack",
    "mcp__triseek__search_content",
    "mcp__triseek__memo_check",
]


CommandRunner = Callable[..., subprocess.CompletedProcess[str]]


def extract_claude_tool_uses(stdout: str) -> list[str]:
    tool_uses: list[str] = []
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        tool_uses.extend(_find_tool_uses(payload))
    return tool_uses


def extract_codex_tool_uses(stdout: str) -> list[str]:
    tool_uses: list[str] = []
    seen: set[tuple[str, str]] = set()
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError:
            continue
        item = payload.get("item") if isinstance(payload, dict) else None
        if not isinstance(item, dict):
            continue
        if payload.get("type") != "item.completed":
            continue
        if item.get("status") == "failed" or item.get("error") is not None:
            continue
        name = item.get("name") or item.get("tool_name")
        if not name and item.get("server") == "triseek" and isinstance(item.get("tool"), str):
            name = f"mcp__triseek__{item['tool']}"
        if not isinstance(name, str) or not name.startswith("mcp__triseek__"):
            continue
        item_id = str(item.get("id") or len(tool_uses))
        key = (item_id, name)
        if key in seen:
            continue
        seen.add(key)
        tool_uses.append(name)
    return tool_uses


def run_agent_smoke(
    *,
    results_root: Path | None = None,
    enable_agent_smoke: bool = False,
    harness: str = "claude",
    command_runner: CommandRunner = subprocess.run,
    triseek_bin: Path | None = None,
    max_budget_usd: float = 1.0,
    manage_daemon: bool = True,
) -> dict[str, Any]:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
    output_root = (results_root or DEFAULT_RESULTS_ROOT / f"agent-smoke-{timestamp}").resolve()
    if not enable_agent_smoke:
        output_root.mkdir(parents=True, exist_ok=True)
        payload = {
            "skipped": True,
            "reason": "agent smoke is optional; pass --enable-agent-smoke to run live-agent flows",
        }
        (output_root / "agent-smoke.json").write_text(json.dumps(payload, indent=2) + "\n")
        return {"output_root": str(output_root), **payload}

    if harness not in {"claude", "codex"}:
        raise ValueError(f"unsupported live agent harness `{harness}`")

    output_root.mkdir(parents=True, exist_ok=True)
    repo_root = build_fixture("best_auth", output_root / "fixture").resolve()
    index_dir = repo_root / ".triseek-index"
    resolved_triseek = (triseek_bin or _resolve_triseek_binary()).resolve()
    harness_env = os.environ.copy()
    harness_env["TRISEEK_HOME"] = str((output_root / "triseek-home").resolve())
    harness_env["TRISEEK_DAEMON_MAX_LOADED_ENGINES"] = "2"
    harness_env["TRISEEK_DAEMON_ENGINE_IDLE_SECS"] = "600"
    mcp_config = _write_claude_mcp_config(
        output_root / "mcp-config.json",
        resolved_triseek,
        repo_root,
        index_dir,
        _mcp_env(harness_env),
    )
    prompt = _agent_prompt()
    if harness == "claude":
        command = _claude_command(mcp_config, max_budget_usd)
        extract_tool_uses = extract_claude_tool_uses
    else:
        command = _codex_command(resolved_triseek, repo_root, index_dir, _mcp_env(harness_env))
        extract_tool_uses = extract_codex_tool_uses
    if manage_daemon:
        _stop_daemon(resolved_triseek, harness_env)
        _start_daemon(resolved_triseek, repo_root, harness_env)

    daemon_status: dict[str, Any] | None = None
    try:
        completed = command_runner(
            command,
            cwd=str(repo_root),
            input=prompt,
            capture_output=True,
            text=True,
            timeout=180,
            env=harness_env,
        )
        if manage_daemon:
            daemon_status = _daemon_status(resolved_triseek, repo_root, harness_env)
    finally:
        if manage_daemon:
            _stop_daemon(resolved_triseek, harness_env)

    (output_root / "agent-stdout.jsonl").write_text(completed.stdout or "", encoding="utf-8")
    (output_root / "agent-stderr.txt").write_text(completed.stderr or "", encoding="utf-8")

    tool_uses = extract_tool_uses(completed.stdout or "")
    missing = [tool for tool in REQUIRED_TOOLS if tool not in tool_uses]
    verdict = "helps" if completed.returncode == 0 and not missing else "hurts"
    run = {
        "id": f"{harness}_live_agent_smoke",
        "group": "agent_smoke",
        "verdict": verdict,
        "hit_at_1": REQUIRED_TOOLS[0] in tool_uses[:1],
        "hit_at_4": all(tool in tool_uses[:4] for tool in REQUIRED_TOOLS[:2]),
        "mrr": 1.0 if REQUIRED_TOOLS[0] in tool_uses else 0.0,
        "oracle_coverage": (len(REQUIRED_TOOLS) - len(missing)) / len(REQUIRED_TOOLS),
        "precision_at_pack": 1.0,
        "misleading_top1": bool(tool_uses and tool_uses[0] != REQUIRED_TOOLS[0]),
        "expansion_needed": False,
        "pack_tokens": 0,
        "baseline_tokens": 1,
        "token_reduction_ratio": 1.0 if verdict == "helps" else 0.0,
        "pack_tool_calls": len(tool_uses),
        "baseline_tool_calls": len(REQUIRED_TOOLS),
        "tool_call_reduction": 0,
        "pack_latency_ms": 0.0,
        "baseline_latency_ms": 0.0,
        "cli_mcp_paths_match": True,
        "harness": harness,
        "repo_root": str(repo_root),
        "returncode": completed.returncode,
        "tool_uses": tool_uses,
        "missing_tool_uses": missing,
        "daemon_status": daemon_status,
    }
    aggregate = write_reports(output_root, [run])
    result = {"output_root": str(output_root), "aggregate": aggregate, "runs": [run]}
    (output_root / "agent-smoke.json").write_text(json.dumps(result, indent=2) + "\n")

    if verdict != "helps":
        raise RuntimeError(
            "live agent smoke failed: "
            f"returncode={run['returncode']} missing_tool_uses={run['missing_tool_uses']}"
        )
    return result


def _find_tool_uses(value: Any) -> list[str]:
    if isinstance(value, dict):
        found: list[str] = []
        if value.get("type") == "tool_use" and isinstance(value.get("name"), str):
            found.append(value["name"])
        for child in value.values():
            found.extend(_find_tool_uses(child))
        return found
    if isinstance(value, list):
        found: list[str] = []
        for child in value:
            found.extend(_find_tool_uses(child))
        return found
    return []


def _resolve_triseek_binary() -> Path:
    for candidate in [
        REPO_ROOT / "target" / "release" / "triseek",
        REPO_ROOT / "target" / "debug" / "triseek",
    ]:
        if candidate.exists():
            return candidate
    which = shutil.which("triseek")
    if which:
        return Path(which)
    raise FileNotFoundError("could not locate triseek binary; build it first")


def _write_claude_mcp_config(
    path: Path,
    triseek_bin: Path,
    repo_root: Path,
    index_dir: Path,
    env: dict[str, str],
) -> Path:
    payload = {
        "mcpServers": {
            "triseek": {
                "command": str(triseek_bin.resolve()),
                "args": [
                    "mcp",
                    "serve",
                    "--repo",
                    str(repo_root),
                    "--index-dir",
                    str(index_dir),
                ],
                "env": env,
            }
        }
    }
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return path


def _claude_command(mcp_config: Path, max_budget_usd: float) -> list[str]:
    return [
        "claude",
        "--bare",
        "--print",
        "--verbose",
        "--output-format",
        "stream-json",
        "--mcp-config",
        str(mcp_config),
        "--strict-mcp-config",
        "--allowedTools",
        ",".join(REQUIRED_TOOLS),
        "--max-budget-usd",
        str(max_budget_usd),
        "--permission-mode",
        "bypassPermissions",
    ]


def _codex_command(
    triseek_bin: Path,
    repo_root: Path,
    index_dir: Path,
    env: dict[str, str],
) -> list[str]:
    mcp_args = [
        "mcp",
        "serve",
        "--repo",
        str(repo_root),
        "--index-dir",
        str(index_dir),
    ]
    return [
        "codex",
        "exec",
        "--json",
        "--ignore-user-config",
        "--skip-git-repo-check",
        "--cd",
        str(repo_root),
        "--dangerously-bypass-approvals-and-sandbox",
        "-c",
        f'mcp_servers.triseek.command="{triseek_bin}"',
        "-c",
        f"mcp_servers.triseek.args={json.dumps(mcp_args)}",
        "-c",
        f"mcp_servers.triseek.env={_toml_inline_table(env)}",
    ]


def _mcp_env(env: dict[str, str]) -> dict[str, str]:
    return {
        "TRISEEK_HOME": env["TRISEEK_HOME"],
        "TRISEEK_DAEMON_MAX_LOADED_ENGINES": env["TRISEEK_DAEMON_MAX_LOADED_ENGINES"],
        "TRISEEK_DAEMON_ENGINE_IDLE_SECS": env["TRISEEK_DAEMON_ENGINE_IDLE_SECS"],
    }


def _toml_inline_table(values: dict[str, str]) -> str:
    parts = [f"{key}={json.dumps(value)}" for key, value in sorted(values.items())]
    return "{" + ",".join(parts) + "}"


def _start_daemon(triseek_bin: Path, repo_root: Path, env: dict[str, str]) -> None:
    subprocess.run(
        [
            str(triseek_bin),
            "daemon",
            "start",
            "--idle-timeout",
            "0",
            str(repo_root),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )


def _stop_daemon(triseek_bin: Path, env: dict[str, str]) -> None:
    subprocess.run(
        [str(triseek_bin), "daemon", "stop"],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )


def _daemon_status(triseek_bin: Path, repo_root: Path, env: dict[str, str]) -> dict[str, Any]:
    completed = subprocess.run(
        [str(triseek_bin), "daemon", "status", str(repo_root)],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    return json.loads(completed.stdout)


def _agent_prompt() -> str:
    return "\n".join(
        [
            "You are running an automated TriSeek live agent harness smoke.",
            "Use the TriSeek MCP server tools only. Do not use shell tools and do not edit files.",
            "Call mcp__triseek__context_pack with goal `fix auth panic for service accounts` and intent `bugfix`.",
            "Then call mcp__triseek__search_content for literal query `auth panic` with limit 5.",
            "Then call mcp__triseek__memo_check for path `src/auth.rs`.",
            "Finish with JSON only: {\"triseek_agent_smoke\":\"ok\"}.",
        ]
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results-root", type=Path)
    parser.add_argument("--enable-agent-smoke", action="store_true")
    parser.add_argument("--harness", default="claude")
    parser.add_argument("--triseek-bin", type=Path)
    parser.add_argument("--max-budget-usd", type=float, default=1.0)
    args = parser.parse_args()
    outcome = run_agent_smoke(
        results_root=args.results_root,
        enable_agent_smoke=args.enable_agent_smoke,
        harness=args.harness,
        triseek_bin=args.triseek_bin,
        max_budget_usd=args.max_budget_usd,
    )
    print(json.dumps(outcome, indent=2))


if __name__ == "__main__":
    main()
