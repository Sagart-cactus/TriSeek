from __future__ import annotations

import csv
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


def aggregate_runs(runs: list[dict[str, Any]]) -> dict[str, Any]:
    verdict_counts = Counter(run["verdict"] for run in runs)
    group_counts: dict[str, Counter[str]] = defaultdict(Counter)
    for run in runs:
        group_counts[run["group"]][run["verdict"]] += 1
    total = len(runs)
    return {
        "total_runs": total,
        "verdict_counts": {
            "helps": verdict_counts.get("helps", 0),
            "neutral": verdict_counts.get("neutral", 0),
            "hurts": verdict_counts.get("hurts", 0),
        },
        "group_verdict_counts": {
            group: {
                "helps": counts.get("helps", 0),
                "neutral": counts.get("neutral", 0),
                "hurts": counts.get("hurts", 0),
            }
            for group, counts in sorted(group_counts.items())
        },
        "hit_at_1_rate": _rate(runs, "hit_at_1"),
        "hit_at_4_rate": _rate(runs, "hit_at_4"),
        "avg_token_reduction_ratio": _avg(runs, "token_reduction_ratio"),
        "avg_tool_call_reduction": _avg(runs, "tool_call_reduction"),
    }


def write_reports(output_root: Path, runs: list[dict[str, Any]]) -> dict[str, Any]:
    output_root.mkdir(parents=True, exist_ok=True)
    aggregate = aggregate_runs(runs)
    (output_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    (output_root / "report.json").write_text(json.dumps({"runs": runs}, indent=2) + "\n")
    (output_root / "summary.md").write_text(render_summary(runs, aggregate))
    (output_root / "public-summary.md").write_text(render_public_summary(runs, aggregate))
    _write_csv(output_root / "report.csv", runs)
    return aggregate


def render_summary(runs: list[dict[str, Any]], aggregate: dict[str, Any]) -> str:
    lines = [
        "# Context Pack Validation Summary",
        "",
        f"- Total runs: {aggregate['total_runs']}",
        f"- Helps / neutral / hurts: {aggregate['verdict_counts']['helps']} / {aggregate['verdict_counts']['neutral']} / {aggregate['verdict_counts']['hurts']}",
        f"- Hit@1: {aggregate['hit_at_1_rate']:.1%}",
        f"- Hit@4: {aggregate['hit_at_4_rate']:.1%}",
        f"- Average token reduction: {aggregate['avg_token_reduction_ratio']:.1%}",
        f"- Average tool-call reduction: {aggregate['avg_tool_call_reduction']:.2f}",
        "",
        "## Runs",
    ]
    for run in runs:
        lines.append(
            f"- {run['id']} ({run['group']}): {run['verdict']} "
            f"hit@4={run['hit_at_4']} pack_tokens={run['pack_tokens']} "
            f"baseline_tokens={run['baseline_tokens']}"
        )
    return "\n".join(lines) + "\n"


def render_public_summary(runs: list[dict[str, Any]], aggregate: dict[str, Any]) -> str:
    helps = [run for run in runs if run["verdict"] == "helps"]
    misses = [run for run in runs if run["verdict"] != "helps"]
    lines = [
        "# How Often Context Packs Help",
        "",
        "This deterministic benchmark measures navigation/context selection, not full task completion.",
        "",
        f"- Scenarios measured: {aggregate['total_runs']}",
        f"- Helped / neutral / hurt: {aggregate['verdict_counts']['helps']} / {aggregate['verdict_counts']['neutral']} / {aggregate['verdict_counts']['hurts']}",
        f"- Required file in default pack (Hit@4): {aggregate['hit_at_4_rate']:.1%}",
        f"- Average token reduction vs scripted search chain: {aggregate['avg_token_reduction_ratio']:.1%}",
        "",
        "## Where It Helps",
    ]
    lines.extend(_example_lines(helps))
    lines.append("")
    lines.append("## Where It Does Not Help")
    lines.extend(_example_lines(misses))
    lines.append("")
    lines.append(
        "Interpretation: context packs are most useful when the goal contains concrete symbols, error text, or changed files. They are less useful for vague, high-frequency, or misleading goals."
    )
    return "\n".join(lines) + "\n"


def _example_lines(runs: list[dict[str, Any]]) -> list[str]:
    if not runs:
        return ["- None in this run."]
    return [
        f"- {run['id']} ({run['group']}): {run['verdict']}, hit@4={run['hit_at_4']}, token_delta={run['baseline_tokens'] - run['pack_tokens']}"
        for run in runs[:5]
    ]


def _write_csv(path: Path, runs: list[dict[str, Any]]) -> None:
    fields = [
        "id",
        "group",
        "verdict",
        "hit_at_1",
        "hit_at_4",
        "mrr",
        "oracle_coverage",
        "precision_at_pack",
        "misleading_top1",
        "expansion_needed",
        "pack_tokens",
        "baseline_tokens",
        "token_reduction_ratio",
        "pack_tool_calls",
        "baseline_tool_calls",
        "tool_call_reduction",
        "pack_latency_ms",
        "baseline_latency_ms",
        "cli_mcp_paths_match",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for run in runs:
            writer.writerow({field: run.get(field) for field in fields})


def _rate(runs: list[dict[str, Any]], key: str) -> float:
    return sum(1 for run in runs if run.get(key)) / len(runs) if runs else 0.0


def _avg(runs: list[dict[str, Any]], key: str) -> float:
    return sum(float(run.get(key, 0.0)) for run in runs) / len(runs) if runs else 0.0
