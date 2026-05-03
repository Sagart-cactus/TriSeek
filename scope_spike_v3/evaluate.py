from __future__ import annotations

import argparse
import json
from pathlib import Path

from scope_spike_v2.evaluate import run_evaluation

DEFAULT_MANIFEST = Path("scope_spike_v3/study_manifest.json")
DEFAULT_GROUND_TRUTH_ROOT = Path("scope_spike_v3/results/captures")
DEFAULT_RESULTS_ROOT = Path("scope_spike_v3/results/evaluation")


def _rewrite_v3_headings(results_root: Path) -> None:
    summary_path = results_root / "summary.md"
    if summary_path.exists():
        text = summary_path.read_text()
        text = text.replace("# Scope Spike V2 Summary", "# Scope Spike V3 Summary")
        summary_path.write_text(text)
    for report_path in results_root.glob("*.report.md"):
        text = report_path.read_text()
        text = text.replace("=== Scope Spike V2 Report ===", "=== Scope Spike V3 Report ===")
        report_path.write_text(text)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Evaluate lexical vs Scope retrieval on the cold-start v3 benchmark."
    )
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST), help="Cold-start study manifest.")
    parser.add_argument(
        "--ground-truth-root",
        default=str(DEFAULT_GROUND_TRUTH_ROOT),
        help="Directory containing the captured v3 per-run reports.",
    )
    parser.add_argument(
        "--results-root",
        help="Directory for evaluation outputs. Defaults to scope_spike_v3/results/evaluation.",
    )
    parser.add_argument("--max-files", type=int, default=10, help="How many files to rank per task.")
    args = parser.parse_args()
    result = run_evaluation(
        manifest_path=Path(args.manifest),
        ground_truth_root=Path(args.ground_truth_root),
        results_root=Path(args.results_root) if args.results_root else DEFAULT_RESULTS_ROOT,
        max_files=args.max_files,
    )
    _rewrite_v3_headings(Path(result["results_root"]))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
