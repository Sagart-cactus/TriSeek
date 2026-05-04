from __future__ import annotations

import argparse
import difflib
import json
import math
import os
import re
from collections import defaultdict
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from statistics import mean

from scope_spike.capture import parse_claude_session
from scope_spike.models import ParsedTrace, ToolCall
from scope_spike.tokenizer import count_tokens, tokenizer_metadata

DEFAULT_REPORT_ROOT = Path("scope_spike/results/study-20260415-rerun")
DEFAULT_RESULTS_ROOT = Path("memo_spike_v2/results")

PATH_LINE_RE = re.compile(r"^(?P<path>.*?):(?P<line>\d+):(?P<body>.*)$")
READ_COMMAND_RE = re.compile(r"\b(cat|head|tail|sed|nl|awk)\b")
SEARCH_COMMAND_RE = re.compile(r"\b(rg|grep|git grep)\b")
LIST_COMMAND_RE = re.compile(r"\b(find|fd|ls)\b")


def _candidate_exists(path: str, repo_root: str | None) -> bool:
    candidate = Path(path)
    if not candidate.is_absolute() and repo_root:
        candidate = Path(repo_root) / candidate
    return candidate.exists()


def _normalize_path(path: str | None, repo_root: str | None) -> str | None:
    if not path:
        return None
    normalized = os.path.normpath(path.strip())
    if normalized.startswith("-"):
        return None
    if any(token in normalized for token in ("<<", ">>", "$(", "${", "*", ";")):
        return None
    if normalized in {"EOF", "PY", "JSON"}:
        return None
    if repo_root and Path(repo_root).exists() and not _candidate_exists(normalized, repo_root):
        return None
    if not repo_root:
        return normalized
    try:
        path_obj = Path(normalized).resolve()
        root_obj = Path(repo_root).resolve()
        return str(path_obj.relative_to(root_obj))
    except Exception:
        return normalized


def _read_content(call: ToolCall) -> str:
    structured = call.result_structured or {}
    file_payload = structured.get("file")
    if isinstance(file_payload, dict) and isinstance(file_payload.get("content"), str):
        return str(file_payload["content"])
    if isinstance(structured.get("content"), str):
        return str(structured["content"])
    if isinstance(structured.get("stdout"), str):
        return str(structured["stdout"])
    return call.result_content or ""


def _extract_single_read_path(command: str) -> str | None:
    patterns = [
        r"\bcat\s+(?P<path>[^\s|;&]+)",
        r"\bhead(?:\s+-[^\s]+)*\s+(?P<path>[^\s|;&]+)",
        r"\btail(?:\s+-[^\s]+)*\s+(?P<path>[^\s|;&]+)",
        r"\bnl(?:\s+-[^\s]+)*\s+(?P<path>[^\s|;&]+)",
        r"\bsed\s+-n\s+['\"][^'\"]+['\"]\s+(?P<path>[^\s|;&]+)",
    ]
    for pattern in patterns:
        match = re.search(pattern, command)
        if match:
            return match.group("path").strip("\"'")
    return None


@dataclass
class NavigationEvent:
    kind: str
    path: str | None
    tokens: int
    partial: bool = False
    tool: str = ""


@dataclass
class EditEvent:
    path: str | None
    diff_tokens: int


@dataclass(frozen=True)
class MemoPolicy:
    name: str
    unchanged_full_floor: int
    unchanged_full_ratio: float
    unchanged_partial_ratio: float
    changed_extra: int
    changed_search_extra: int
    cached_search_floor: int

    def unchanged_full_cost(self, original_tokens: int) -> int:
        return min(
            original_tokens,
            max(self.unchanged_full_floor, math.ceil(original_tokens * self.unchanged_full_ratio)),
        )

    def unchanged_partial_cost(self, original_tokens: int) -> int:
        return min(original_tokens, max(24, math.ceil(original_tokens * self.unchanged_partial_ratio)))

    def changed_read_cost(self, original_tokens: int, diff_tokens: int) -> int:
        return min(original_tokens, diff_tokens + self.changed_extra)

    def cached_search_cost(self, original_tokens: int) -> int:
        return min(original_tokens, self.cached_search_floor)

    def changed_search_cost(self, original_tokens: int, diff_tokens: int) -> int:
        return min(original_tokens, diff_tokens + self.changed_search_extra)


CONSERVATIVE = MemoPolicy(
    name="conservative",
    unchanged_full_floor=96,
    unchanged_full_ratio=0.25,
    unchanged_partial_ratio=0.60,
    changed_extra=96,
    changed_search_extra=64,
    cached_search_floor=48,
)

DEFAULT = MemoPolicy(
    name="default",
    unchanged_full_floor=64,
    unchanged_full_ratio=0.12,
    unchanged_partial_ratio=0.40,
    changed_extra=48,
    changed_search_extra=32,
    cached_search_floor=24,
)

AGGRESSIVE = MemoPolicy(
    name="aggressive",
    unchanged_full_floor=32,
    unchanged_full_ratio=0.08,
    unchanged_partial_ratio=0.28,
    changed_extra=32,
    changed_search_extra=20,
    cached_search_floor=16,
)


@dataclass
class CacheState:
    cached: bool = False
    dirty: bool = False
    pending_diff_tokens: int = 0


def _parse_search_lines(content: str, repo_root: str | None, tool_name: str) -> list[NavigationEvent]:
    buckets: dict[str | None, int] = defaultdict(int)
    for raw_line in content.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("[Showing results"):
            continue
        if line.startswith("<system-reminder>") or line.startswith("</system-reminder>"):
            continue
        match = PATH_LINE_RE.match(line)
        if match:
            path = _normalize_path(match.group("path"), repo_root)
            buckets[path] += count_tokens(line)
            continue
        if "/" in line or line.endswith((".rs", ".py", ".md", ".toml", ".yml", ".yaml")):
            path = _normalize_path(line, repo_root)
            buckets[path] += count_tokens(line)
            continue
        buckets[None] += count_tokens(line)
    return [
        NavigationEvent(kind="search", path=path, tokens=tokens, tool=tool_name)
        for path, tokens in buckets.items()
    ]


def _events_from_call(call: ToolCall, repo_root: str | None) -> list[NavigationEvent | EditEvent]:
    events: list[NavigationEvent | EditEvent] = []
    if call.name in {"Edit", "MultiEdit", "Write"}:
        path = call.input.get("file_path")
        if not path and call.result_structured:
            path = call.result_structured.get("filePath")
        normalized = _normalize_path(str(path), repo_root) if path else None
        if call.name == "Write":
            diff_tokens = count_tokens(str(call.input.get("content") or ""))
        elif call.name == "MultiEdit":
            diff_tokens = 0
            for edit in call.input.get("edits", []) or []:
                old = str(edit.get("old_string") or "")
                new = str(edit.get("new_string") or "")
                diff = "".join(
                    difflib.unified_diff(
                        old.splitlines(keepends=True),
                        new.splitlines(keepends=True),
                        lineterm="",
                    )
                )
                diff_tokens += count_tokens(diff)
        else:
            old = str(call.input.get("old_string") or "")
            new = str(call.input.get("new_string") or "")
            diff = "".join(
                difflib.unified_diff(
                    old.splitlines(keepends=True),
                    new.splitlines(keepends=True),
                    lineterm="",
                )
            )
            diff_tokens = count_tokens(diff)
        events.append(EditEvent(path=normalized, diff_tokens=max(1, diff_tokens)))
        return events

    if call.name == "Read":
        structured = call.result_structured or {}
        file_payload = structured.get("file") if isinstance(structured.get("file"), dict) else None
        path = None
        if file_payload and file_payload.get("filePath"):
            path = file_payload.get("filePath")
        elif call.input.get("file_path"):
            path = call.input.get("file_path")
        normalized = _normalize_path(str(path), repo_root) if path else None
        if normalized:
            partial = "offset" in call.input or "limit" in call.input
            events.append(
                NavigationEvent(
                    kind="read",
                    path=normalized,
                    tokens=count_tokens(_read_content(call)),
                    partial=partial,
                    tool="Read",
                )
            )
        return events

    if call.name == "Grep":
        return _parse_search_lines(_read_content(call), repo_root, "Grep")

    if call.name == "Glob":
        return _parse_search_lines(_read_content(call), repo_root, "Glob")

    if call.name == "LS":
        return _parse_search_lines(_read_content(call), repo_root, "LS")

    if call.name == "Bash":
        command = str(call.input.get("command") or "")
        content = _read_content(call)
        if not command or not content:
            return events
        if SEARCH_COMMAND_RE.search(command):
            return _parse_search_lines(content, repo_root, "Bash")
        if LIST_COMMAND_RE.search(command):
            return _parse_search_lines(content, repo_root, "Bash")
        if READ_COMMAND_RE.search(command):
            path = _extract_single_read_path(command)
            normalized = _normalize_path(path, repo_root) if path else None
            if normalized:
                partial = not re.search(r"\bcat\s+", command)
                events.append(
                    NavigationEvent(
                        kind="read",
                        path=normalized,
                        tokens=count_tokens(content),
                        partial=partial,
                        tool="Bash",
                    )
                )
        return events

    return events


def _iter_events(parsed: ParsedTrace) -> list[NavigationEvent | EditEvent]:
    repo_root = parsed.cwd
    events: list[NavigationEvent | EditEvent] = []
    for call in parsed.tool_calls:
        events.extend(_events_from_call(call, repo_root))
    return events


def simulate_trace(parsed: ParsedTrace, policy: MemoPolicy) -> dict[str, object]:
    events = _iter_events(parsed)
    actual_navigation_tokens = 0
    simulated_navigation_tokens = 0
    cache: dict[str, CacheState] = defaultdict(CacheState)
    unchanged_rereads = 0
    changed_rereads = 0
    cached_searches = 0
    changed_searches = 0
    first_reads = 0
    passthrough_tokens = 0
    per_file_saved: dict[str, int] = defaultdict(int)

    for event in events:
        if isinstance(event, EditEvent):
            if event.path and cache[event.path].cached:
                cache[event.path].dirty = True
                cache[event.path].pending_diff_tokens += event.diff_tokens
            continue

        actual_navigation_tokens += event.tokens
        simulated_cost = event.tokens

        if event.path is None or event.kind == "listing":
            passthrough_tokens += event.tokens
            simulated_navigation_tokens += simulated_cost
            continue

        state = cache[event.path]
        if event.kind == "read":
            if not state.cached:
                first_reads += 1
                state.cached = True
            elif state.dirty:
                changed_rereads += 1
                simulated_cost = policy.changed_read_cost(event.tokens, state.pending_diff_tokens)
                state.dirty = False
                state.pending_diff_tokens = 0
            else:
                unchanged_rereads += 1
                simulated_cost = (
                    policy.unchanged_partial_cost(event.tokens)
                    if event.partial
                    else policy.unchanged_full_cost(event.tokens)
                )
        elif event.kind == "search":
            if state.cached and state.dirty:
                changed_searches += 1
                simulated_cost = policy.changed_search_cost(event.tokens, state.pending_diff_tokens)
            elif state.cached:
                cached_searches += 1
                simulated_cost = policy.cached_search_cost(event.tokens)

        simulated_navigation_tokens += simulated_cost
        per_file_saved[event.path] += event.tokens - simulated_cost

    saved_navigation_tokens = actual_navigation_tokens - simulated_navigation_tokens
    total_tokens = parsed.assistant_output_tokens + actual_navigation_tokens
    simulated_total_tokens = parsed.assistant_output_tokens + simulated_navigation_tokens
    top_saved_files = sorted(per_file_saved.items(), key=lambda item: (-item[1], item[0]))[:5]

    return {
        "policy": policy.name,
        "actual_navigation_tokens": actual_navigation_tokens,
        "memo_navigation_tokens": simulated_navigation_tokens,
        "saved_navigation_tokens": saved_navigation_tokens,
        "navigation_reduction_ratio": (
            saved_navigation_tokens / actual_navigation_tokens if actual_navigation_tokens else 0.0
        ),
        "actual_total_tokens": total_tokens,
        "memo_total_tokens": simulated_total_tokens,
        "saved_total_tokens": total_tokens - simulated_total_tokens,
        "total_reduction_ratio": (
            (total_tokens - simulated_total_tokens) / total_tokens if total_tokens else 0.0
        ),
        "first_reads": first_reads,
        "unchanged_rereads": unchanged_rereads,
        "changed_rereads": changed_rereads,
        "cached_searches": cached_searches,
        "changed_searches": changed_searches,
        "passthrough_tokens": passthrough_tokens,
        "top_saved_files": [
            {"path": path, "saved_tokens": saved}
            for path, saved in top_saved_files
            if saved > 0
        ],
    }


def _verdict(total_reduction_ratio: float) -> str:
    if total_reduction_ratio >= 0.20:
        return "PASS"
    if total_reduction_ratio >= 0.10:
        return "MARGINAL"
    return "FAIL"


def render_run_report(payload: dict[str, object]) -> str:
    lines = [
        "=== Memo Spike V2 Report ===",
        f"Run: {payload['run_id']}",
        f"Task: {payload['task']}",
        f"Trace: {payload['trace_path']}",
        f"Tokenizer: {tokenizer_metadata()['name']}",
        "",
    ]
    for policy in payload["policies"]:
        lines.extend(
            [
                f"{policy['policy'].upper()}:",
                f"  Navigation reduction: {policy['navigation_reduction_ratio']:.1%}",
                f"  Total-token reduction: {policy['total_reduction_ratio']:.1%}",
                f"  Unchanged rereads:     {policy['unchanged_rereads']}",
                f"  Changed rereads:       {policy['changed_rereads']}",
                f"  Cached searches:       {policy['cached_searches']}",
                f"  Goal-relative verdict: {_verdict(policy['total_reduction_ratio'])}",
                "",
            ]
        )
    return "\n".join(lines)


def render_summary(aggregate: dict[str, object], runs: list[dict[str, object]], study_label: str) -> str:
    lines = [
        f"# Memo Spike V2 Summary ({study_label})",
        "",
        f"- Total runs: {aggregate['total_runs']}",
    ]
    for profile_name in ("conservative", "default", "aggressive"):
        profile = aggregate["profiles"][profile_name]
        lines.extend(
            [
                f"- {profile_name} avg navigation reduction: {profile['avg_navigation_reduction_ratio']:.1%}",
                f"- {profile_name} avg total-token reduction: {profile['avg_total_reduction_ratio']:.1%}",
                f"- {profile_name} goal-relative verdict: {profile['verdict']}",
            ]
        )
    lines.extend(["", "## Runs"])
    for run in runs:
        default_profile = next(item for item in run["policies"] if item["policy"] == "default")
        lines.append(
            f"- {run['run_id']}: default total reduction {default_profile['total_reduction_ratio']:.1%}, "
            f"default navigation reduction {default_profile['navigation_reduction_ratio']:.1%}"
        )
    lines.append("")
    return "\n".join(lines)


def aggregate_results(runs: list[dict[str, object]]) -> dict[str, object]:
    profiles: dict[str, dict[str, float | str]] = {}
    for profile_name in ("conservative", "default", "aggressive"):
        matching = [
            profile
            for run in runs
            for profile in run["policies"]
            if profile["policy"] == profile_name
        ]
        avg_total = mean(profile["total_reduction_ratio"] for profile in matching) if matching else 0.0
        profiles[profile_name] = {
            "avg_navigation_reduction_ratio": mean(
                profile["navigation_reduction_ratio"] for profile in matching
            )
            if matching
            else 0.0,
            "avg_total_reduction_ratio": avg_total,
            "verdict": _verdict(avg_total),
        }
    return {
        "total_runs": len(runs),
        "profiles": profiles,
    }


def run_study(
    *,
    report_root: Path = DEFAULT_REPORT_ROOT,
    results_root: Path | None = None,
) -> dict[str, object]:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    output_root = (results_root or DEFAULT_RESULTS_ROOT / f"study-{timestamp}").resolve()
    output_root.mkdir(parents=True, exist_ok=True)

    reports = sorted(report_root.glob("*.report.json"))
    runs: list[dict[str, object]] = []
    for report_path in reports:
        report = json.loads(report_path.read_text())
        parsed = parse_claude_session(report["actual"]["trace_path"])
        payload = {
            "run_id": report["label"],
            "task": report["task"],
            "trace_path": report["actual"]["trace_path"],
            "policies": [
                simulate_trace(parsed, CONSERVATIVE),
                simulate_trace(parsed, DEFAULT),
                simulate_trace(parsed, AGGRESSIVE),
            ],
        }
        (output_root / f"{report['label']}.report.json").write_text(json.dumps(payload, indent=2) + "\n")
        (output_root / f"{report['label']}.report.md").write_text(render_run_report(payload))
        runs.append(payload)

    aggregate = aggregate_results(runs)
    (output_root / "aggregate.json").write_text(json.dumps(aggregate, indent=2) + "\n")
    (output_root / "summary.md").write_text(render_summary(aggregate, runs, output_root.name))
    return {
        "results_root": str(output_root),
        "aggregate": aggregate,
        "summary_path": str(output_root / "summary.md"),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Run a realistic Memo cache evaluation over stored traces.")
    parser.add_argument(
        "--report-root",
        default=str(DEFAULT_REPORT_ROOT),
        help="Directory containing the original scope_spike per-run reports.",
    )
    parser.add_argument(
        "--results-root",
        help="Directory to write results into. Defaults to a timestamped directory under memo_spike_v2/results.",
    )
    args = parser.parse_args()
    result = run_study(
        report_root=Path(args.report_root),
        results_root=Path(args.results_root) if args.results_root else None,
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

