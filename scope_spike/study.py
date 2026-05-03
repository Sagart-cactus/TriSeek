from __future__ import annotations

import argparse
import json
import shutil
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from statistics import mean, median

from scope_spike.capture import ClaudeAuthError, run_claude_capture
from scope_spike.spike_runner import run_trace


@dataclass(frozen=True)
class StudyRunSpec:
    run_id: str
    repo_slug: str
    repo_path: str
    task_type: str
    title: str
    prompt: str
    verification_command: str


def load_manifest(path: str | Path = "scope_spike/study_manifest.json") -> list[StudyRunSpec]:
    raw_items = json.loads(Path(path).read_text())
    return [StudyRunSpec(**item) for item in raw_items]


def prepare_workdir(source_repo: str | Path, destination: str | Path) -> Path:
    src = Path(source_repo)
    dst = Path(destination)
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(
        src,
        dst,
        ignore=shutil.ignore_patterns(".git", "target", "__pycache__", "*.pyc"),
    )
    return dst


def aggregate_reports(reports: list[dict[str, object]]) -> dict[str, object]:
    ratios = [float(report["oracle"]["elimination_ratio"]) for report in reports]
    pass_count = sum(1 for ratio in ratios if ratio > 0.80)
    average_ratio = mean(ratios) if ratios else 0.0
    if average_ratio > 0.80:
        verdict = "PASS"
    elif average_ratio >= 0.66:
        verdict = "MARGINAL"
    else:
        verdict = "FAIL"
    return {
        "avg_elimination_ratio": average_ratio,
        "median_elimination_ratio": median(ratios) if ratios else 0.0,
        "min_elimination_ratio": min(ratios) if ratios else 0.0,
        "max_elimination_ratio": max(ratios) if ratios else 0.0,
        "total_runs": len(reports),
        "pass_count": pass_count,
        "verdict": verdict,
    }


def render_summary(
    *,
    study_label: str,
    aggregate: dict[str, object],
    reports: list[dict[str, object]],
) -> str:
    lines = [
        f"# Scope Spike Summary ({study_label})",
        "",
        f"- Total runs: {aggregate['total_runs']}",
        f"- Average elimination ratio: {aggregate['avg_elimination_ratio']:.1%}",
        f"- Median elimination ratio: {aggregate['median_elimination_ratio']:.1%}",
        f"- Range: {aggregate['min_elimination_ratio']:.1%} to {aggregate['max_elimination_ratio']:.1%}",
        f"- PASS-count (>80% elimination): {aggregate['pass_count']}",
        f"- Verdict: {aggregate['verdict']}",
        "",
        "## Runs",
    ]
    for report in reports:
        lines.append(
            f"- {report['label']}: {report['oracle']['verdict']} "
            f"({report['oracle']['elimination_ratio']:.1%} elimination, "
            f"{report['oracle']['reduction_factor'] if report['oracle']['reduction_factor'] is not None else 'n/a'}x reduction)"
        )
    lines.append("")
    return "\n".join(lines)


def render_blocked_summary(*, study_label: str, message: str) -> str:
    return "\n".join(
        [
            f"# Scope Spike Summary ({study_label})",
            "",
            "Study status: blocked",
            "",
            "The fresh 6-run study could not start because the local `claude` CLI",
            "failed authentication during the first capture attempt.",
            "",
            f"Blocker: {message}",
            "",
            "No study verdict was produced from fresh runs.",
            "",
        ]
    )


def run_study(
    *,
    manifest_path: str | Path = "scope_spike/study_manifest.json",
    results_root: str | Path | None = None,
    model: str | None = None,
) -> dict[str, object]:
    manifest = load_manifest(manifest_path)
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    results_root = Path(results_root or f"scope_spike/results/study-{timestamp}")
    results_root.mkdir(parents=True, exist_ok=True)

    reports: list[dict[str, object]] = []
    manifest_copy = [item.__dict__ for item in manifest]
    (results_root / "manifest.json").write_text(json.dumps(manifest_copy, indent=2) + "\n")

    for spec in manifest:
        workdir = prepare_workdir(spec.repo_path, Path("scope_spike/workdirs") / spec.run_id)
        try:
            capture = run_claude_capture(
                spec.prompt,
                workdir,
                session_id=None,
                model=model,
            )
        except ClaudeAuthError as exc:
            message = str(exc)
            (results_root / "capture-error.txt").write_text(message + "\n")
            aggregate = {
                "status": "blocked",
                "error": message,
                "blocked_run": spec.run_id,
                "completed_runs": len(reports),
                "total_runs": len(manifest),
            }
            (results_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
            (results_root / "summary.md").write_text(
                render_blocked_summary(study_label=results_root.name, message=message)
            )
            return {
                "results_root": str(results_root),
                "aggregate": aggregate,
                "summary_path": str(results_root / "summary.md"),
                "blocked": True,
            }

        run_result = run_trace(
            trace_path=capture["trace_path"],
            task_description=spec.title,
            label=spec.run_id,
            results_dir=results_root,
        )
        reports.append(run_result["payload"])

    aggregate = aggregate_reports(reports)
    summary_text = render_summary(
        study_label=results_root.name,
        aggregate=aggregate,
        reports=reports,
    )
    (results_root / "summary.md").write_text(summary_text)
    (results_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    return {
        "results_root": str(results_root),
        "aggregate": aggregate,
        "summary_path": str(results_root / "summary.md"),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the standalone Scope spike study.")
    parser.add_argument(
        "--manifest",
        default="scope_spike/study_manifest.json",
        help="Study manifest describing the six benchmark runs.",
    )
    parser.add_argument(
        "--results-root",
        help="Directory to write study outputs into. Defaults to a timestamped results directory.",
    )
    parser.add_argument("--model", help="Optional Claude model override for fresh captures.")
    args = parser.parse_args()

    result = run_study(
        manifest_path=args.manifest,
        results_root=args.results_root,
        model=args.model,
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
