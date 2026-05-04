from __future__ import annotations

import argparse
import json
from pathlib import Path

from memo_spike_v2.study import run_study

DEFAULT_REPORT_ROOT = Path("scope_spike_v3/results/captures")
DEFAULT_RESULTS_ROOT = Path("memo_spike_v3/results/study")


def _rewrite_v3_headings(results_root: Path) -> None:
    summary_path = results_root / "summary.md"
    if summary_path.exists():
        text = summary_path.read_text()
        text = text.replace("# Memo Spike V2 Summary", "# Memo Spike V3 Summary")
        summary_path.write_text(text)
    for report_path in results_root.glob("*.report.md"):
        text = report_path.read_text()
        text = text.replace("=== Memo Spike V2 Report ===", "=== Memo Spike V3 Report ===")
        report_path.write_text(text)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the Memo v3 simulation on cold-start traces.")
    parser.add_argument(
        "--report-root",
        default=str(DEFAULT_REPORT_ROOT),
        help="Directory containing the captured v3 per-run reports.",
    )
    parser.add_argument(
        "--results-root",
        help="Directory for Memo v3 outputs. Defaults to memo_spike_v3/results/study.",
    )
    args = parser.parse_args()
    result = run_study(
        report_root=Path(args.report_root),
        results_root=Path(args.results_root) if args.results_root else DEFAULT_RESULTS_ROOT,
    )
    _rewrite_v3_headings(Path(result["results_root"]))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
