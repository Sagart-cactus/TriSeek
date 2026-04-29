from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Oracle:
    required_files: list[str]
    helpful_files: list[str] | None = None
    bad_files: list[str] | None = None

    @property
    def useful_files(self) -> set[str]:
        return set(self.required_files) | set(self.helpful_files or [])


@dataclass
class RunComparison:
    hit_at_1: bool
    hit_at_4: bool
    mrr: float
    oracle_coverage: float
    precision_at_pack: float
    misleading_top1: bool
    expansion_needed: bool
    pack_tokens: int
    baseline_tokens: int
    token_reduction_ratio: float
    pack_tool_calls: int
    baseline_tool_calls: int
    tool_call_reduction: int
    pack_latency_ms: float
    baseline_latency_ms: float
    latency_ratio: float
    verdict: str


def estimate_tokens(value: str) -> int:
    return max(1, (len(value) + 3) // 4)


def score_pack(
    pack_paths: list[str],
    oracle: Oracle,
    *,
    pack_tokens: int,
    baseline_tokens: int,
    pack_latency_ms: float,
    baseline_latency_ms: float,
    pack_tool_calls: int,
    baseline_tool_calls: int,
) -> RunComparison:
    required = set(oracle.required_files)
    useful = oracle.useful_files
    bad = set(oracle.bad_files or [])
    first_required_rank = 0
    for index, path in enumerate(pack_paths, start=1):
        if path in required:
            first_required_rank = index
            break

    hit_at_1 = bool(pack_paths) and pack_paths[0] in required
    hit_at_4 = any(path in required for path in pack_paths[:4])
    mrr = 1.0 / first_required_rank if first_required_rank else 0.0
    covered = len(required.intersection(pack_paths)) + len(
        set(oracle.helpful_files or []).intersection(pack_paths)
    )
    denominator = max(1, len(required) + len(oracle.helpful_files or []))
    oracle_coverage = covered / denominator
    precision_at_pack = (
        len(useful.intersection(pack_paths)) / len(pack_paths) if pack_paths else 0.0
    )
    misleading_top1 = bool(pack_paths) and pack_paths[0] in bad
    expansion_needed = not hit_at_4
    token_reduction_ratio = (
        (baseline_tokens - pack_tokens) / baseline_tokens if baseline_tokens else 0.0
    )
    latency_ratio = pack_latency_ms / baseline_latency_ms if baseline_latency_ms else 0.0
    comparison = RunComparison(
        hit_at_1=hit_at_1,
        hit_at_4=hit_at_4,
        mrr=mrr,
        oracle_coverage=oracle_coverage,
        precision_at_pack=precision_at_pack,
        misleading_top1=misleading_top1,
        expansion_needed=expansion_needed,
        pack_tokens=pack_tokens,
        baseline_tokens=baseline_tokens,
        token_reduction_ratio=token_reduction_ratio,
        pack_tool_calls=pack_tool_calls,
        baseline_tool_calls=baseline_tool_calls,
        tool_call_reduction=baseline_tool_calls - pack_tool_calls,
        pack_latency_ms=pack_latency_ms,
        baseline_latency_ms=baseline_latency_ms,
        latency_ratio=latency_ratio,
        verdict="",
    )
    comparison.verdict = classify_verdict(comparison)
    return comparison


def classify_verdict(comparison: RunComparison) -> str:
    if comparison.misleading_top1:
        return "hurts"
    if comparison.hit_at_4 and (
        comparison.token_reduction_ratio > 0.15 or comparison.tool_call_reduction >= 1
    ):
        return "helps"
    if comparison.expansion_needed and comparison.pack_tokens > comparison.baseline_tokens:
        return "hurts"
    return "neutral"
