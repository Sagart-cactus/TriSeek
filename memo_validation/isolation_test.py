"""Session isolation test for Memo Phase 5 validation (success criterion 3).

Replays two traces with different session IDs:
  1. Sequentially (A then B)
  2. Concurrently (A and B interleaved via threading)

Asserts that the ReplayResult values are identical, proving that concurrent
sessions do not interfere with each other.

Usage (daemon must be running):
    python -m memo_validation.isolation_test
"""
from __future__ import annotations

import sys
import threading
import uuid
from pathlib import Path

from scope_spike.capture import parse_claude_session

from memo_validation.replay import ReplayResult, replay_trace
from memo_validation.rpc_client import MemoRpcClient

REPO_ROOT = Path(__file__).parent.parent

# Use two different explicit-prompt traces for the isolation test.
# They share the same workdir structure (serde repo) so the daemon will
# track different file sets under different session IDs.
TRACE_REPORTS = [
    REPO_ROOT
    / "scope_spike"
    / "results"
    / "study-20260415-rerun"
    / "serde-bugfix-lowercase-screaming-rule-aliases.report.json",
    REPO_ROOT
    / "scope_spike"
    / "results"
    / "study-20260415-rerun"
    / "serde-feature-train-case-rule.report.json",
]


def _load_trace(report_path: Path):
    import json

    report = json.loads(report_path.read_text())
    return parse_claude_session(report["actual"]["trace_path"])


def _run_sequential(client: MemoRpcClient) -> tuple[ReplayResult, ReplayResult]:
    traces = [_load_trace(p) for p in TRACE_REPORTS]
    sid_a = f"isolation-seq-a-{uuid.uuid4()}"
    sid_b = f"isolation-seq-b-{uuid.uuid4()}"
    result_a = replay_trace(traces[0], client, run_id="isolation-a", session_id=sid_a)
    result_b = replay_trace(traces[1], client, run_id="isolation-b", session_id=sid_b)
    return result_a, result_b


def _run_concurrent(client: MemoRpcClient) -> tuple[ReplayResult, ReplayResult]:
    traces = [_load_trace(p) for p in TRACE_REPORTS]
    results: dict[str, ReplayResult] = {}
    sid_a = f"isolation-par-a-{uuid.uuid4()}"
    sid_b = f"isolation-par-b-{uuid.uuid4()}"

    def run_a():
        results["a"] = replay_trace(traces[0], client, run_id="isolation-a", session_id=sid_a)

    def run_b():
        results["b"] = replay_trace(traces[1], client, run_id="isolation-b", session_id=sid_b)

    t_a = threading.Thread(target=run_a)
    t_b = threading.Thread(target=run_b)
    t_a.start()
    t_b.start()
    t_a.join()
    t_b.join()
    return results["a"], results["b"]


def _assert_results_equal(
    seq: ReplayResult,
    par: ReplayResult,
    label: str,
) -> None:
    failures = []
    for attr in (
        "total_reads",
        "redundant_reads_prevented",
        "tokens_saved",
        "false_negatives",
        "actual_navigation_tokens",
        "memo_navigation_tokens",
    ):
        sv = getattr(seq, attr)
        pv = getattr(par, attr)
        if sv != pv:
            failures.append(f"  {attr}: sequential={sv} concurrent={pv}")
    if failures:
        raise AssertionError(
            f"Session isolation failure for {label}:\n" + "\n".join(failures)
        )


def run_isolation_test(daemon_port: int | None = None) -> bool:
    """Run the isolation test. Returns True if all assertions pass."""
    client = MemoRpcClient(port=daemon_port)

    print("Running sequential replay...")
    seq_a, seq_b = _run_sequential(client)

    print("Running concurrent replay...")
    par_a, par_b = _run_concurrent(client)

    print("\nComparing results...")
    all_pass = True
    for label, seq, par in [("trace-A", seq_a, par_a), ("trace-B", seq_b, par_b)]:
        try:
            _assert_results_equal(seq, par, label)
            print(f"  {label}: PASS")
        except AssertionError as e:
            print(f"  {label}: FAIL\n{e}")
            all_pass = False

    # Also check zero false negatives across all runs
    for label, result in [("seq-A", seq_a), ("seq-B", seq_b), ("par-A", par_a), ("par-B", par_b)]:
        if result.false_negatives > 0:
            print(f"  {label}: FAIL — false_negatives={result.false_negatives}")
            all_pass = False
        else:
            print(f"  {label}: false_negatives=0 OK")

    if all_pass:
        print("\nIsolation test: PASS")
    else:
        print("\nIsolation test: FAIL")
    return all_pass


def main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description="Run the Memo session isolation test.")
    parser.add_argument("--daemon-port", type=int, default=None)
    args = parser.parse_args()

    ok = run_isolation_test(daemon_port=args.daemon_port)
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
