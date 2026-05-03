from __future__ import annotations

import argparse
import json
from pathlib import Path

from scope_spike.analyze import analyze_trace
from scope_spike.capture import parse_claude_session
from scope_spike.oracle import build_oracle
from scope_spike.report import default_output_stem, generate_report, make_report_payload


def run_trace(
    *,
    trace_path: str | Path,
    task_description: str,
    label: str | None = None,
    results_dir: str | Path = "scope_spike/results",
) -> dict[str, object]:
    parsed = parse_claude_session(trace_path)
    analysis = analyze_trace(parsed)
    oracle = build_oracle(analysis)
    label = label or default_output_stem(Path(trace_path).stem, str(trace_path))
    payload = make_report_payload(
        task_description=task_description,
        label=label,
        trace_analysis=analysis,
        oracle=oracle,
    )
    report_text = generate_report(
        task_description=task_description,
        label=label,
        trace_analysis=analysis,
        oracle=oracle,
    )

    output_root = Path(results_dir)
    output_root.mkdir(parents=True, exist_ok=True)
    stem = default_output_stem(label, str(trace_path))
    json_path = output_root / f"{stem}.report.json"
    md_path = output_root / f"{stem}.report.md"
    json_path.write_text(json.dumps(payload, indent=2) + "\n")
    md_path.write_text(report_text)

    return {
        "payload": payload,
        "report_text": report_text,
        "json_path": str(json_path),
        "md_path": str(md_path),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Analyze a Claude trace for Scope-spike metrics.")
    parser.add_argument("--trace", required=True, help="Path to a Claude Code JSONL session log.")
    parser.add_argument("--task", required=True, help="Human-readable task description.")
    parser.add_argument("--label", help="Short label for this run.")
    parser.add_argument(
        "--results-dir",
        default="scope_spike/results",
        help="Directory to write report artifacts into.",
    )
    args = parser.parse_args()

    result = run_trace(
        trace_path=args.trace,
        task_description=args.task,
        label=args.label,
        results_dir=args.results_dir,
    )
    print(result["report_text"], end="")
    print(f"JSON report: {result['json_path']}")
    print(f"Markdown report: {result['md_path']}")


if __name__ == "__main__":
    main()

