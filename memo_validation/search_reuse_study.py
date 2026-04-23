"""Combined validation study for TriSeek search-result reuse plus Memo file rereads."""
from __future__ import annotations

import argparse
import csv
import json
import uuid
from datetime import UTC, datetime
from pathlib import Path
from statistics import mean
from typing import Any

from scope_spike.capture import parse_claude_session

from memo_validation.replay import replay_trace
from memo_validation.rpc_client import MemoRpcClient
from memo_validation.search_reuse_replay import replay_search_trace

REPO_ROOT = Path(__file__).parent.parent
DEFAULT_EXPLICIT_ORACLE = REPO_ROOT / "scope_spike" / "results" / "study-20260415-rerun"
DEFAULT_COLDSTART_ORACLE = (
    REPO_ROOT / "scope_spike_v3" / "results" / "captures-20260415-coldstart"
)
DEFAULT_RESULTS_ROOT = REPO_ROOT / "memo_validation" / "results"
DEFAULT_BASELINE_RUN = DEFAULT_RESULTS_ROOT / "study-20260415T164935Z"


def _load_report(report_path: Path) -> dict[str, Any]:
    return json.loads(report_path.read_text())


def _run_verdict(payload: dict[str, Any]) -> str:
    file_false_negatives = payload["file_replay"]["false_negatives"]
    search_false_negatives = payload["search_replay"]["search_false_negatives"]
    if file_false_negatives or search_false_negatives:
        return "FAIL"
    return "PASS"


def run_single(
    oracle_report: dict[str, Any],
    client: MemoRpcClient,
    *,
    compact_at_turn: int | None = None,
) -> dict[str, Any]:
    trace_path = Path(oracle_report["actual"]["trace_path"])
    label = oracle_report.get("label", trace_path.stem)
    parsed = parse_claude_session(trace_path)

    file_session_id = str(uuid.uuid4())
    file_result = replay_trace(
        parsed,
        client,
        run_id=label,
        session_id=file_session_id,
        compact_at_turn=compact_at_turn,
    )

    search_session_id = str(uuid.uuid4())
    search_result = replay_search_trace(
        parsed,
        client,
        run_id=label,
        session_id=search_session_id,
        compact_at_turn=compact_at_turn,
    )

    payload: dict[str, Any] = {
        "run_id": label,
        "task": oracle_report.get("task", ""),
        "trace_path": str(trace_path),
        "compact_at_turn": compact_at_turn,
        "file_replay": {
            "session_id": file_result.session_id,
            "tokens_saved": file_result.tokens_saved,
            "eligible_tokens": file_result.memo_eligible_redundant_tokens,
            "false_negatives": file_result.false_negatives,
            "post_compact_false_negatives": file_result.post_compact_false_negatives,
            "actual_navigation_tokens": file_result.actual_navigation_tokens,
            "optimized_navigation_tokens": file_result.memo_navigation_tokens,
        },
        "search_replay": {
            "session_id": search_result.session_id,
            "total_search_calls": search_result.total_search_calls,
            "duplicate_search_calls": search_result.duplicate_search_calls,
            "search_reuse_hits": search_result.search_reuse_hits,
            "search_tokens_saved": search_result.search_tokens_saved,
            "search_eligible_tokens": search_result.search_eligible_tokens,
            "search_false_negatives": search_result.search_false_negatives,
            "post_compact_false_negatives": search_result.post_compact_false_negatives,
            "actual_search_tokens": search_result.actual_search_tokens,
            "replay_search_tokens": search_result.replay_search_tokens,
        },
    }
    payload["combined"] = {
        "actual_navigation_tokens": payload["file_replay"]["actual_navigation_tokens"]
        + payload["search_replay"]["actual_search_tokens"],
        "optimized_navigation_tokens": payload["file_replay"]["optimized_navigation_tokens"]
        + payload["search_replay"]["replay_search_tokens"],
        "combined_eligible_tokens": payload["file_replay"]["eligible_tokens"]
        + payload["search_replay"]["search_eligible_tokens"],
        "combined_tokens_saved": payload["file_replay"]["tokens_saved"]
        + payload["search_replay"]["search_tokens_saved"],
    }
    payload["verdict"] = _run_verdict(payload)
    return payload


def aggregate_results(runs: list[dict[str, Any]]) -> dict[str, Any]:
    file_saved = sum(run["file_replay"]["tokens_saved"] for run in runs)
    file_eligible = sum(run["file_replay"]["eligible_tokens"] for run in runs)
    search_saved = sum(run["search_replay"]["search_tokens_saved"] for run in runs)
    search_eligible = sum(run["search_replay"]["search_eligible_tokens"] for run in runs)
    combined_saved = sum(run["combined"]["combined_tokens_saved"] for run in runs)
    combined_eligible = sum(run["combined"]["combined_eligible_tokens"] for run in runs)
    search_hits = sum(run["search_replay"]["search_reuse_hits"] for run in runs)
    duplicate_calls = sum(run["search_replay"]["duplicate_search_calls"] for run in runs)
    overall_combined_ratio = combined_saved / combined_eligible if combined_eligible else 0.0
    per_run_ratios = [
        (run["combined"]["combined_tokens_saved"] / run["combined"]["combined_eligible_tokens"])
        for run in runs
        if run["combined"]["combined_eligible_tokens"] > 0
    ]
    return {
        "total_runs": len(runs),
        "file_false_negatives_total": sum(run["file_replay"]["false_negatives"] for run in runs),
        "search_false_negatives_total": sum(
            run["search_replay"]["search_false_negatives"] for run in runs
        ),
        "post_compact_false_negatives_total": sum(
            run["file_replay"]["post_compact_false_negatives"]
            + run["search_replay"]["post_compact_false_negatives"]
            for run in runs
        ),
        "file_read_eligible_tokens": file_eligible,
        "file_read_tokens_saved": file_saved,
        "search_eligible_tokens": search_eligible,
        "search_tokens_saved": search_saved,
        "combined_eligible_tokens": combined_eligible,
        "combined_tokens_saved": combined_saved,
        "search_reuse_hits": search_hits,
        "duplicate_search_calls": duplicate_calls,
        "overall_combined_reduction_ratio": overall_combined_ratio,
        "avg_per_run_combined_reduction_ratio": mean(per_run_ratios) if per_run_ratios else 0.0,
        "pass_count": sum(1 for run in runs if run["verdict"] == "PASS"),
        "fail_count": sum(1 for run in runs if run["verdict"] == "FAIL"),
    }


def render_summary(runs: list[dict[str, Any]], aggregate: dict[str, Any], label: str) -> str:
    lines = [
        f"# Search Reuse Validation Summary ({label})",
        "",
        f"- Total runs:                    {aggregate['total_runs']}",
        f"- File false negatives:         {aggregate['file_false_negatives_total']}",
        f"- Search false negatives:       {aggregate['search_false_negatives_total']}",
        f"- Post-compact false negatives: {aggregate['post_compact_false_negatives_total']}",
        f"- File saved / eligible:        {aggregate['file_read_tokens_saved']} / {aggregate['file_read_eligible_tokens']}",
        f"- Search saved / eligible:      {aggregate['search_tokens_saved']} / {aggregate['search_eligible_tokens']}",
        f"- Combined saved / eligible:    {aggregate['combined_tokens_saved']} / {aggregate['combined_eligible_tokens']}",
        f"- Duplicate search hits:        {aggregate['search_reuse_hits']} / {aggregate['duplicate_search_calls']}",
        f"- Combined reduction ratio:     {aggregate['overall_combined_reduction_ratio']:.1%}",
        f"- PASS / FAIL:                  {aggregate['pass_count']} / {aggregate['fail_count']}",
        "",
        "## Per-run",
    ]
    for run in runs:
        lines.append(
            f"- {run['run_id']}: {run['verdict']}  "
            f"file_saved={run['file_replay']['tokens_saved']} "
            f"search_saved={run['search_replay']['search_tokens_saved']} "
            f"search_hits={run['search_replay']['search_reuse_hits']} "
            f"combined_saved={run['combined']['combined_tokens_saved']}"
        )
    return "\n".join(lines)


def load_baseline_summary(baseline_run: Path) -> dict[str, Any] | None:
    aggregate_path = baseline_run / "aggregate.json"
    if not aggregate_path.exists():
        return None
    return json.loads(aggregate_path.read_text())


def render_comparison(
    aggregate: dict[str, Any],
    baseline: dict[str, Any] | None,
    baseline_label: str,
) -> str:
    lines = [
        "# Comparison",
        "",
        f"- Current combined saved tokens: {aggregate['combined_tokens_saved']}",
        f"- Current file saved tokens:     {aggregate['file_read_tokens_saved']}",
        f"- Current search saved tokens:   {aggregate['search_tokens_saved']}",
    ]
    if baseline is None:
        lines.append(f"- Baseline `{baseline_label}` not found.")
        return "\n".join(lines)

    baseline_saved = baseline.get("total_tokens_saved", 0)
    baseline_eligible = baseline.get("total_memo_eligible_redundant_tokens", 0)
    lines.extend(
        [
            f"- Baseline file-only saved tokens ({baseline_label}): {baseline_saved}",
            f"- Baseline file-only eligible tokens ({baseline_label}): {baseline_eligible}",
            f"- Added search savings vs baseline: {aggregate['combined_tokens_saved'] - baseline_saved}",
        ]
    )
    return "\n".join(lines)


def write_csv(output_root: Path, runs: list[dict[str, Any]]) -> None:
    fieldnames = [
        "run_id",
        "verdict",
        "file_tokens_saved",
        "file_eligible_tokens",
        "file_false_negatives",
        "search_tokens_saved",
        "search_eligible_tokens",
        "search_false_negatives",
        "search_reuse_hits",
        "duplicate_search_calls",
        "combined_tokens_saved",
        "combined_eligible_tokens",
    ]
    with (output_root / "report.csv").open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        for run in runs:
            writer.writerow(
                {
                    "run_id": run["run_id"],
                    "verdict": run["verdict"],
                    "file_tokens_saved": run["file_replay"]["tokens_saved"],
                    "file_eligible_tokens": run["file_replay"]["eligible_tokens"],
                    "file_false_negatives": run["file_replay"]["false_negatives"],
                    "search_tokens_saved": run["search_replay"]["search_tokens_saved"],
                    "search_eligible_tokens": run["search_replay"]["search_eligible_tokens"],
                    "search_false_negatives": run["search_replay"]["search_false_negatives"],
                    "search_reuse_hits": run["search_replay"]["search_reuse_hits"],
                    "duplicate_search_calls": run["search_replay"]["duplicate_search_calls"],
                    "combined_tokens_saved": run["combined"]["combined_tokens_saved"],
                    "combined_eligible_tokens": run["combined"]["combined_eligible_tokens"],
                }
            )


def run_study(
    *,
    explicit_root: Path = DEFAULT_EXPLICIT_ORACLE,
    coldstart_root: Path = DEFAULT_COLDSTART_ORACLE,
    results_root: Path | None = None,
    baseline_run: Path = DEFAULT_BASELINE_RUN,
    daemon_port: int | None = None,
    compact_at_turn: int | None = None,
    max_runs: int | None = None,
) -> dict[str, Any]:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    default_name = (
        f"search-reuse-{timestamp}-compact"
        if compact_at_turn is not None
        else f"search-reuse-{timestamp}"
    )
    output_root = (results_root or DEFAULT_RESULTS_ROOT / default_name).resolve()
    output_root.mkdir(parents=True, exist_ok=True)

    client = MemoRpcClient(port=daemon_port)
    runs: list[dict[str, Any]] = []
    for root in [explicit_root, coldstart_root]:
        for report_path in sorted(root.glob("*.report.json")):
            if max_runs is not None and len(runs) >= max_runs:
                break
            payload = run_single(
                _load_report(report_path),
                client,
                compact_at_turn=compact_at_turn,
            )
            (output_root / f"{payload['run_id']}.report.json").write_text(
                json.dumps(payload, indent=2) + "\n"
            )
            runs.append(payload)
            print(
                f"  {payload['run_id']}: {payload['verdict']} "
                f"file_saved={payload['file_replay']['tokens_saved']} "
                f"search_saved={payload['search_replay']['search_tokens_saved']}"
            )
        if max_runs is not None and len(runs) >= max_runs:
            break

    aggregate = aggregate_results(runs)
    (output_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    (output_root / "summary.md").write_text(render_summary(runs, aggregate, output_root.name))
    (output_root / "comparison.md").write_text(
        render_comparison(aggregate, load_baseline_summary(baseline_run), baseline_run.name)
    )
    write_csv(output_root, runs)
    return {"output_root": str(output_root), "aggregate": aggregate}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--explicit-root", type=Path, default=DEFAULT_EXPLICIT_ORACLE)
    parser.add_argument("--coldstart-root", type=Path, default=DEFAULT_COLDSTART_ORACLE)
    parser.add_argument("--results-root", type=Path)
    parser.add_argument("--baseline-run", type=Path, default=DEFAULT_BASELINE_RUN)
    parser.add_argument("--daemon-port", type=int)
    parser.add_argument("--compact-at-turn", type=int)
    parser.add_argument("--max-runs", type=int)
    args = parser.parse_args()

    outcome = run_study(
        explicit_root=args.explicit_root,
        coldstart_root=args.coldstart_root,
        results_root=args.results_root,
        baseline_run=args.baseline_run,
        daemon_port=args.daemon_port,
        compact_at_turn=args.compact_at_turn,
        max_runs=args.max_runs,
    )
    print(json.dumps(outcome["aggregate"], indent=2))
    print(f"Wrote results to {outcome['output_root']}")


if __name__ == "__main__":
    main()
