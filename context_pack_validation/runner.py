from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from context_pack_validation.fixtures import build_fixture
from context_pack_validation.metrics import Oracle, estimate_tokens, score_pack
from context_pack_validation.report import write_reports
from memo_validation.mcp_client import TriseekMcpClient, resolve_triseek_binary

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_SCENARIOS = REPO_ROOT / "context_pack_validation" / "scenarios.yaml"
DEFAULT_RESULTS_ROOT = REPO_ROOT / "context_pack_validation" / "results"


@dataclass(frozen=True)
class Scenario:
    id: str
    group: str
    repo: str
    agent_prompt: str
    goal: str
    intent: str
    oracle: Oracle
    baseline_steps: list[dict[str, Any]]
    changed_files: list[str]


def load_scenarios(path: Path = DEFAULT_SCENARIOS) -> list[Scenario]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    scenarios = []
    for raw in payload.get("scenarios", []):
        for key in [
            "id",
            "group",
            "repo",
            "agent_prompt",
            "goal",
            "intent",
            "oracle",
            "baseline_steps",
        ]:
            if key not in raw:
                raise ValueError(f"scenario is missing `{key}`: {raw}")
        oracle_raw = raw["oracle"]
        scenarios.append(
            Scenario(
                id=raw["id"],
                group=raw["group"],
                repo=raw["repo"],
                agent_prompt=raw["agent_prompt"],
                goal=raw["goal"],
                intent=raw["intent"],
                oracle=Oracle(
                    required_files=list(oracle_raw.get("required_files") or []),
                    helpful_files=list(oracle_raw.get("helpful_files") or []),
                    bad_files=list(oracle_raw.get("bad_files") or []),
                ),
                baseline_steps=list(raw["baseline_steps"]),
                changed_files=list(raw.get("changed_files") or []),
            )
        )
    return scenarios


def run_validation(
    *,
    scenarios_path: Path = DEFAULT_SCENARIOS,
    results_root: Path | None = None,
    repo_limit: int | None = None,
    scenario_filter: str | None = None,
    triseek_bin: Path | None = None,
) -> dict[str, Any]:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
    output_root = (results_root or DEFAULT_RESULTS_ROOT / f"run-{timestamp}").resolve()
    triseek = triseek_bin or _ensure_triseek_binary()
    scenarios = _filter_scenarios(load_scenarios(scenarios_path), repo_limit, scenario_filter)
    runs = []
    with tempfile.TemporaryDirectory() as tmp:
        fixture_root = Path(tmp) / "fixtures"
        for scenario in scenarios:
            runs.append(_run_scenario(scenario, fixture_root, triseek))
    aggregate = write_reports(output_root, runs)
    return {"output_root": str(output_root), "aggregate": aggregate, "runs": runs}


def _run_scenario(scenario: Scenario, fixture_root: Path, triseek: Path) -> dict[str, Any]:
    repo_root = _resolve_repo(scenario, fixture_root)
    _build_index(triseek, repo_root)

    cli_pack, cli_latency = _run_cli_context_pack(triseek, repo_root, scenario)
    with TriseekMcpClient(repo_root, binary=triseek) as client:
        tool_description = _context_pack_tool_description(client)
        mcp_pack, mcp_latency = _run_mcp_context_pack(client, scenario)
        baseline, baseline_latency = _run_baseline(client, scenario)
        _run_search_regression(client)

    cli_paths = _paths_from_pack(cli_pack)
    mcp_paths = _paths_from_pack(mcp_pack)
    pack_tokens = int(cli_pack.get("estimated_tokens") or estimate_tokens(json.dumps(cli_pack)))
    baseline_tokens = sum(item["tokens"] for item in baseline)
    comparison = score_pack(
        cli_paths,
        scenario.oracle,
        pack_tokens=pack_tokens,
        baseline_tokens=baseline_tokens,
        pack_latency_ms=cli_latency + mcp_latency,
        baseline_latency_ms=baseline_latency,
        pack_tool_calls=1,
        baseline_tool_calls=len(scenario.baseline_steps),
    )

    return {
        "id": scenario.id,
        "group": scenario.group,
        "repo": scenario.repo,
        "agent_prompt": scenario.agent_prompt,
        "context_pack_instruction": context_pack_instruction(scenario),
        "mcp_tool_description": tool_description,
        "mcp_call_arguments": context_pack_arguments(scenario),
        "goal": scenario.goal,
        "intent": scenario.intent,
        "cli_paths": cli_paths,
        "mcp_paths": mcp_paths,
        "cli_mcp_paths_match": cli_paths == mcp_paths,
        **comparison.__dict__,
    }


def _resolve_repo(scenario: Scenario, fixture_root: Path) -> Path:
    if scenario.repo.startswith("fixture:"):
        return build_fixture(scenario.repo.split(":", 1)[1], fixture_root)
    path = Path(scenario.repo).expanduser()
    if not path.exists():
        raise FileNotFoundError(f"scenario repo does not exist: {scenario.repo}")
    return path.resolve()


def _build_index(triseek: Path, repo_root: Path) -> None:
    subprocess.run(
        [str(triseek), "build", "--json", str(repo_root)],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )


def _run_cli_context_pack(
    triseek: Path,
    repo_root: Path,
    scenario: Scenario,
) -> tuple[dict[str, Any], float]:
    command = [
        str(triseek),
        "context-pack",
        "--json",
        "--goal",
        scenario.goal,
        "--intent",
        scenario.intent,
        str(repo_root),
    ]
    for changed_file in scenario.changed_files:
        command.extend(["--changed-file", changed_file])
    started = time.perf_counter()
    completed = subprocess.run(command, check=True, capture_output=True, text=True)
    latency = (time.perf_counter() - started) * 1000.0
    return json.loads(completed.stdout), latency


def _run_mcp_context_pack(
    client: TriseekMcpClient,
    scenario: Scenario,
) -> tuple[dict[str, Any], float]:
    arguments = context_pack_arguments(scenario)
    started = time.perf_counter()
    result = client.call_tool("context_pack", arguments)
    latency = (time.perf_counter() - started) * 1000.0
    if result["is_error"]:
        raise RuntimeError(f"MCP context_pack failed: {result}")
    return result["structured_content"], latency


def context_pack_arguments(scenario: Scenario) -> dict[str, Any]:
    arguments: dict[str, Any] = {"goal": scenario.goal, "intent": scenario.intent}
    if scenario.changed_files:
        arguments["changed_files"] = scenario.changed_files
    return arguments


def context_pack_instruction(scenario: Scenario) -> str:
    changed = (
        f" Include changed_files={scenario.changed_files}."
        if scenario.changed_files
        else ""
    )
    return (
        "Before chaining broad searches, call TriSeek MCP tool `context_pack` "
        f"with goal={scenario.goal!r}, intent={scenario.intent!r}.{changed} "
        "Use the returned ranked files as a small starting set, then expand only if needed."
    )


def _context_pack_tool_description(client: TriseekMcpClient) -> str:
    for tool in client.list_tools():
        if tool.get("name") == "context_pack":
            description = str(tool.get("description") or "")
            if "bounded" not in description:
                raise RuntimeError(f"context_pack tool description is not guidance-rich: {tool}")
            return description
    raise RuntimeError("MCP tools/list did not include context_pack")


def _run_baseline(
    client: TriseekMcpClient,
    scenario: Scenario,
) -> tuple[list[dict[str, Any]], float]:
    outputs = []
    started = time.perf_counter()
    for step in scenario.baseline_steps:
        result = client.call_tool(step["tool"], dict(step["arguments"]))
        if result["is_error"]:
            raise RuntimeError(f"baseline tool failed for {scenario.id}: {result}")
        text = result["content_text"] + json.dumps(result["structured_content"], sort_keys=True)
        outputs.append(
            {
                "tool": step["tool"],
                "tokens": estimate_tokens(text),
                "paths": _paths_from_search(result["structured_content"]),
            }
        )
    return outputs, (time.perf_counter() - started) * 1000.0


def _run_search_regression(client: TriseekMcpClient) -> None:
    client.call_tool("find_files", {"query": "src", "limit": 5})
    client.call_tool("search_content", {"query": "pub", "mode": "literal", "limit": 5})


def _paths_from_pack(pack: dict[str, Any]) -> list[str]:
    return [item["path"] for item in pack.get("items", []) if isinstance(item, dict) and "path" in item]


def _paths_from_search(envelope: dict[str, Any]) -> list[str]:
    return [
        item["path"]
        for item in envelope.get("results", [])
        if isinstance(item, dict) and isinstance(item.get("path"), str)
    ]


def _filter_scenarios(
    scenarios: list[Scenario],
    repo_limit: int | None,
    scenario_filter: str | None,
) -> list[Scenario]:
    if scenario_filter:
        scenarios = [
            scenario
            for scenario in scenarios
            if scenario_filter in scenario.id or scenario_filter == scenario.group
        ]
    if repo_limit is not None:
        scenarios = scenarios[:repo_limit]
    return scenarios


def _ensure_triseek_binary() -> Path:
    try:
        return resolve_triseek_binary()
    except FileNotFoundError:
        subprocess.run(["cargo", "build", "-p", "triseek", "--locked"], check=True, cwd=REPO_ROOT)
        return resolve_triseek_binary()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenarios", type=Path, default=DEFAULT_SCENARIOS)
    parser.add_argument("--results-root", type=Path)
    parser.add_argument("--repo-limit", type=int)
    parser.add_argument("--scenario-filter")
    parser.add_argument("--triseek-bin", type=Path)
    args = parser.parse_args()
    outcome = run_validation(
        scenarios_path=args.scenarios,
        results_root=args.results_root,
        repo_limit=args.repo_limit,
        scenario_filter=args.scenario_filter,
        triseek_bin=args.triseek_bin,
    )
    print(json.dumps(outcome["aggregate"], indent=2))
    print(f"Wrote results to {outcome['output_root']}")


if __name__ == "__main__":
    main()
