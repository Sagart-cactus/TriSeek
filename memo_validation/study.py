"""Memo Phase 5 Validation Study.

Replays all 12 traced Claude Code sessions through the live TriSeek Memo daemon
and produces per-run and aggregate comparison reports.

Usage (daemon must be running first):
    python -m memo_validation.study

Or with custom paths:
    python -m memo_validation.study --explicit-root scope_spike/results/study-20260415-rerun \\
        --coldstart-root scope_spike_v3/results/captures-20260415-coldstart \\
        --results-root memo_validation/results/my-run
"""
from __future__ import annotations

import argparse
import json
import uuid
from datetime import UTC, datetime
from pathlib import Path
from statistics import mean
from typing import Any

from scope_spike.capture import parse_claude_session

from memo_validation.replay import ReplayResult, replay_trace
from memo_validation.rpc_client import MemoRpcClient

REPO_ROOT = Path(__file__).parent.parent

DEFAULT_EXPLICIT_ORACLE = REPO_ROOT / "scope_spike" / "results" / "study-20260415-rerun"
DEFAULT_COLDSTART_ORACLE = (
    REPO_ROOT / "scope_spike_v3" / "results" / "captures-20260415-coldstart"
)
DEFAULT_EXPLICIT_SIM = REPO_ROOT / "memo_spike_v2" / "results" / "study-20260415"
DEFAULT_COLDSTART_SIM = (
    REPO_ROOT / "memo_spike_v3" / "results" / "study-20260415-coldstart"
)
DEFAULT_RESULTS_ROOT = REPO_ROOT / "memo_validation" / "results"


def _load_oracle_report(report_path: Path) -> dict:
    return json.loads(report_path.read_text())


def _load_sim_report(report_path: Path) -> dict | None:
    if report_path.exists():
        return json.loads(report_path.read_text())
    return None


def _get_sim_default(sim_report: dict | None) -> dict | None:
    """Extract the 'default' policy from a memo_spike simulation report."""
    if sim_report is None:
        return None
    for policy in sim_report.get("policies", []):
        if policy.get("policy") == "default":
            return policy
    return None


def _redundant_read_tokens_from_oracle(oracle_report: dict) -> int:
    """Total redundant re-read tokens from the oracle report."""
    return oracle_report.get("actual", {}).get("redundant_read_tokens", 0)


def _run_verdict(result: ReplayResult) -> str:
    """PASS if Memo prevented ≥80% of Memo-eligible redundant re-read tokens and zero false negatives.

    Memo-eligible tokens = re-reads via Read/Bash-cat with no intervening edit.
    The oracle's redundant_read_tokens also counts Grep/Glob→Read and after-edit
    re-reads that Memo cannot intercept, so it is not the right denominator.
    """
    if result.false_negatives > 0:
        return "FAIL"
    if result.memo_eligible_redundant_tokens == 0:
        return "PASS"
    ratio = result.tokens_saved / result.memo_eligible_redundant_tokens
    return "PASS" if ratio >= 0.80 else "MARGINAL" if ratio >= 0.50 else "FAIL"


def run_single(
    oracle_report: dict,
    sim_report: dict | None,
    client: MemoRpcClient,
    *,
    run_id: str | None = None,
    compact_at_turn: int | None = None,
) -> dict:
    """Replay one trace and return the per-run result payload."""
    trace_path = oracle_report["actual"]["trace_path"]
    label = oracle_report.get("label", run_id or Path(trace_path).stem)
    parsed = parse_claude_session(trace_path)

    session_id = str(uuid.uuid4())
    result: ReplayResult = replay_trace(
        parsed,
        client,
        run_id=label,
        session_id=session_id,
        compact_at_turn=compact_at_turn,
    )

    oracle_redundant_tokens = _redundant_read_tokens_from_oracle(oracle_report)
    sim_default = _get_sim_default(sim_report)

    # Ratio vs oracle (informational — oracle includes Grep/Glob→Read which Memo can't intercept)
    oracle_reduction = (
        result.tokens_saved / oracle_redundant_tokens if oracle_redundant_tokens > 0 else None
    )
    # Ratio vs Memo-eligible (correct denominator for the ≥80% success criterion)
    eligible_reduction = (
        result.tokens_saved / result.memo_eligible_redundant_tokens
        if result.memo_eligible_redundant_tokens > 0
        else None
    )

    payload: dict[str, Any] = {
        "run_id": label,
        "task": oracle_report.get("task", ""),
        "trace_path": trace_path,
        "rust_replay": {
            "session_id": result.session_id,
            "total_reads": result.total_reads,
            "redundant_reads_prevented": result.redundant_reads_prevented,
            "tokens_saved": result.tokens_saved,
            "false_negatives": result.false_negatives,
            "post_compact_false_negatives": result.post_compact_false_negatives,
            "actual_navigation_tokens": result.actual_navigation_tokens,
            "memo_navigation_tokens": result.memo_navigation_tokens,
            "memo_eligible_redundant_tokens": result.memo_eligible_redundant_tokens,
            "reduction_ratio_vs_eligible": eligible_reduction,
            "reduction_ratio_vs_oracle": oracle_reduction,
        },
        "oracle": {
            "elimination_ratio": oracle_report.get("oracle", {}).get("elimination_ratio"),
            "eliminated_tokens": oracle_report.get("oracle", {}).get("eliminated_tokens"),
            "redundant_read_tokens": oracle_redundant_tokens,
        },
        "python_simulation": {
            "saved_navigation_tokens": sim_default.get("saved_navigation_tokens") if sim_default else None,
            "navigation_reduction_ratio": sim_default.get("navigation_reduction_ratio") if sim_default else None,
        },
        "compact_at_turn": compact_at_turn,
        "verdict": _run_verdict(result),
    }
    return payload


def aggregate_results(runs: list[dict]) -> dict:
    false_negs = sum(r["rust_replay"]["false_negatives"] for r in runs)
    post_compact_false_negs = sum(r["rust_replay"]["post_compact_false_negatives"] for r in runs)
    eligible_ratios = [
        r["rust_replay"]["reduction_ratio_vs_eligible"]
        for r in runs
        if r["rust_replay"]["reduction_ratio_vs_eligible"] is not None
    ]
    avg_eligible_ratio = mean(eligible_ratios) if eligible_ratios else 0.0
    total_eligible = sum(r["rust_replay"]["memo_eligible_redundant_tokens"] for r in runs)
    total_saved = sum(r["rust_replay"]["tokens_saved"] for r in runs)
    overall_eligible_ratio = total_saved / total_eligible if total_eligible > 0 else 0.0
    return {
        "total_runs": len(runs),
        "false_negatives_total": false_negs,
        "compact_run_false_negatives_total": post_compact_false_negs,
        "total_memo_eligible_redundant_tokens": total_eligible,
        "total_tokens_saved": total_saved,
        "overall_eligible_reduction_ratio": overall_eligible_ratio,
        "avg_per_run_eligible_reduction_ratio": avg_eligible_ratio,
        "meets_80pct_target": overall_eligible_ratio >= 0.80,
        "pass_count": sum(1 for r in runs if r["verdict"] == "PASS"),
        "marginal_count": sum(1 for r in runs if r["verdict"] == "MARGINAL"),
        "fail_count": sum(1 for r in runs if r["verdict"] == "FAIL"),
    }


def render_summary(runs: list[dict], aggregate: dict, label: str) -> str:
    lines = [
        f"# Memo Phase 5 Validation Summary ({label})",
        "",
        f"- Total runs:            {aggregate['total_runs']}",
        f"- False negatives:       {aggregate['false_negatives_total']}",
        f"- Post-compact false negatives: {aggregate['compact_run_false_negatives_total']}",
        f"- Memo-eligible tokens:  {aggregate['total_memo_eligible_redundant_tokens']} saved={aggregate['total_tokens_saved']}",
        f"- Overall eligible ratio:{aggregate['overall_eligible_reduction_ratio']:.1%}",
        f"- Meets ≥80% target:     {aggregate['meets_80pct_target']}",
        f"- PASS / MARGINAL / FAIL: {aggregate['pass_count']} / {aggregate['marginal_count']} / {aggregate['fail_count']}",
        "",
        "## Per-run",
    ]
    for run in runs:
        rr = run["rust_replay"]
        ratio = rr["reduction_ratio_vs_eligible"]
        ratio_str = f"{ratio:.1%}" if ratio is not None else "n/a (no eligible re-reads)"
        oracle_ratio = rr["reduction_ratio_vs_oracle"]
        oracle_str = f"{oracle_ratio:.1%}" if oracle_ratio is not None else "n/a"
        lines.append(
            f"- {run['run_id']}: {run['verdict']}  "
            f"saved={rr['tokens_saved']} eligible={rr['memo_eligible_redundant_tokens']} "
            f"reduction_vs_eligible={ratio_str} (vs_oracle={oracle_str}) "
            f"false_negs={rr['false_negatives']} post_compact_false_negs={rr['post_compact_false_negatives']}"
        )
    return "\n".join(lines)


def run_study(
    *,
    explicit_oracle_root: Path = DEFAULT_EXPLICIT_ORACLE,
    coldstart_oracle_root: Path = DEFAULT_COLDSTART_ORACLE,
    explicit_sim_root: Path = DEFAULT_EXPLICIT_SIM,
    coldstart_sim_root: Path = DEFAULT_COLDSTART_SIM,
    results_root: Path | None = None,
    daemon_port: int | None = None,
    compact_at_turn: int | None = None,
) -> dict:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    default_name = f"study-{timestamp}-compact" if compact_at_turn is not None else f"study-{timestamp}"
    output_root = (results_root or DEFAULT_RESULTS_ROOT / default_name).resolve()
    output_root.mkdir(parents=True, exist_ok=True)

    client = MemoRpcClient(port=daemon_port)

    oracle_reports: list[tuple[Path, Path | None]] = []
    for root, sim_root in [
        (explicit_oracle_root, explicit_sim_root),
        (coldstart_oracle_root, coldstart_sim_root),
    ]:
        for oracle_path in sorted(root.glob("*.report.json")):
            sim_path = sim_root / oracle_path.name if sim_root else None
            oracle_reports.append((oracle_path, sim_path))

    runs: list[dict] = []
    for oracle_path, sim_path in oracle_reports:
        oracle_report = _load_oracle_report(oracle_path)
        sim_report = _load_sim_report(sim_path) if sim_path else None

        run_payload = run_single(
            oracle_report,
            sim_report,
            client,
            compact_at_turn=compact_at_turn,
        )
        run_id = run_payload["run_id"]
        out_path = output_root / f"{run_id}.report.json"
        out_path.write_text(json.dumps(run_payload, indent=2) + "\n")
        runs.append(run_payload)
        verdict = run_payload["verdict"]
        fn = run_payload["rust_replay"]["false_negatives"]
        compact_fn = run_payload["rust_replay"]["post_compact_false_negatives"]
        saved = run_payload["rust_replay"]["tokens_saved"]
        print(
            f"  {run_id}: {verdict}  saved={saved} false_negs={fn} "
            f"post_compact_false_negs={compact_fn}"
        )

    aggregate = aggregate_results(runs)
    (output_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    summary = render_summary(runs, aggregate, output_root.name)
    (output_root / "summary.md").write_text(summary)

    print("\n" + summary)
    return {
        "results_root": str(output_root),
        "aggregate": aggregate,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the Memo Phase 5 validation study.")
    parser.add_argument("--explicit-root", default=str(DEFAULT_EXPLICIT_ORACLE))
    parser.add_argument("--coldstart-root", default=str(DEFAULT_COLDSTART_ORACLE))
    parser.add_argument("--explicit-sim-root", default=str(DEFAULT_EXPLICIT_SIM))
    parser.add_argument("--coldstart-sim-root", default=str(DEFAULT_COLDSTART_SIM))
    parser.add_argument("--results-root", default=None)
    parser.add_argument("--daemon-port", type=int, default=None)
    parser.add_argument("--compact-at-turn", type=int, default=None)
    args = parser.parse_args()

    result = run_study(
        explicit_oracle_root=Path(args.explicit_root),
        coldstart_oracle_root=Path(args.coldstart_root),
        explicit_sim_root=Path(args.explicit_sim_root),
        coldstart_sim_root=Path(args.coldstart_sim_root),
        results_root=Path(args.results_root) if args.results_root else None,
        daemon_port=args.daemon_port,
        compact_at_turn=args.compact_at_turn,
    )
    print(json.dumps(result["aggregate"], indent=2))


if __name__ == "__main__":
    main()
