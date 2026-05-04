from __future__ import annotations

from pathlib import Path

from scope_spike.tokenizer import tokenizer_metadata


def _pct(numerator: int | float, denominator: int | float) -> str:
    if not denominator:
        return "0.0%"
    return f"{(numerator / denominator) * 100:.1f}%"


def _top_files(file_details: list[dict[str, object]], *, useful: bool) -> list[str]:
    rows = [item for item in file_details if bool(item["was_useful"]) is useful]
    rows = sorted(rows, key=lambda item: (-int(item["tokens"]), str(item["path"])))
    return [
        f"- {item['path']} ({item['tokens']} tokens, {item['read_count']} reads)"
        for item in rows[:5]
    ]


def make_report_payload(
    *,
    task_description: str,
    label: str,
    trace_analysis: dict[str, object],
    oracle: dict[str, object],
) -> dict[str, object]:
    return {
        "label": label,
        "task": task_description,
        "repository": trace_analysis.get("repo_root"),
        "tokenizer": tokenizer_metadata(),
        "actual": trace_analysis,
        "oracle": oracle,
        "verdict": oracle["verdict"],
    }


def generate_report(
    *,
    task_description: str,
    label: str,
    trace_analysis: dict[str, object],
    oracle: dict[str, object],
) -> str:
    navigation_tokens = int(trace_analysis["navigation_tokens"])
    total_tokens = int(trace_analysis["total_tokens"])
    useful_lines = _top_files(trace_analysis["file_details"], useful=True)
    wasted_lines = _top_files(trace_analysis["file_details"], useful=False)

    parts = [
        "=== Scope Validation Report ===",
        f"Label: {label}",
        f"Task: {task_description}",
        f"Repository: {trace_analysis.get('repo_root')}",
        f"Tokenizer: {tokenizer_metadata()['name']}",
        "",
        "ACTUAL SESSION:",
        f"  Total tokens:        {total_tokens}",
        f"  Reasoning tokens:    {trace_analysis['reasoning_tokens']}",
        f"  Navigation tokens:   {navigation_tokens} ({_pct(navigation_tokens, total_tokens)})",
        f"  - Wasted reads:      {trace_analysis['wasted_read_tokens']} ({_pct(trace_analysis['wasted_read_tokens'], navigation_tokens)})",
        f"  - Redundant reads:   {trace_analysis['redundant_read_tokens']} ({_pct(trace_analysis['redundant_read_tokens'], navigation_tokens)})",
        f"  - Useful reads:      {trace_analysis['useful_read_tokens']} ({_pct(trace_analysis['useful_read_tokens'], navigation_tokens)})",
        f"  - Overhead/no-path:  {trace_analysis['navigation_overhead_tokens']} ({_pct(trace_analysis['navigation_overhead_tokens'], navigation_tokens)})",
        f"  Files read:          {trace_analysis['files_read']}",
        f"  Files useful:        {trace_analysis['files_useful']}",
        f"  Files wasted:        {trace_analysis['files_wasted']}",
        "",
        "ORACLE (perfect Scope):",
        f"  Navigation tokens:   {oracle['recommended_tokens']}",
        f"  Token savings:       {oracle['eliminated_tokens']} ({oracle['elimination_ratio']:.1%} reduction)",
        f"  Reduction factor:    {oracle['reduction_factor'] if oracle['reduction_factor'] is not None else 'n/a'}",
        "",
        f"VERDICT: {oracle['verdict']}",
        "  >5x reduction  -> PASS",
        "  3-5x reduction -> MARGINAL",
        "  <3x reduction  -> FAIL",
    ]
    if useful_lines:
        parts.extend(["", "TOP USEFUL FILES:", *useful_lines])
    if wasted_lines:
        parts.extend(["", "TOP WASTED FILES:", *wasted_lines])
    return "\n".join(parts) + "\n"


def default_output_stem(label: str, trace_path: str) -> str:
    sanitized = "".join(ch if ch.isalnum() or ch in {"-", "_"} else "-" for ch in label).strip("-")
    if sanitized:
        return sanitized
    return Path(trace_path).stem

