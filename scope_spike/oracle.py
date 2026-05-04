from __future__ import annotations


def _verdict_for_ratio(elimination_ratio: float) -> str:
    if elimination_ratio > 0.80:
        return "PASS"
    if elimination_ratio >= 0.66:
        return "MARGINAL"
    return "FAIL"


def build_oracle(trace_analysis: dict[str, object]) -> dict[str, object]:
    file_details = trace_analysis["file_details"]
    useful_files = [item for item in file_details if item["was_useful"]]
    actual_navigation_tokens = int(trace_analysis["navigation_tokens"])
    oracle_tokens = sum(int(item["first_read_tokens"]) for item in useful_files)
    eliminated_tokens = max(0, actual_navigation_tokens - oracle_tokens)
    elimination_ratio = (
        eliminated_tokens / actual_navigation_tokens if actual_navigation_tokens > 0 else 0.0
    )
    reduction_factor = (
        round(actual_navigation_tokens / oracle_tokens, 2)
        if oracle_tokens > 0
        else None
    )

    return {
        "recommended_files": [item["path"] for item in useful_files],
        "recommended_tokens": oracle_tokens,
        "eliminated_tokens": eliminated_tokens,
        "elimination_ratio": elimination_ratio,
        "reduction_factor": reduction_factor,
        "verdict": _verdict_for_ratio(elimination_ratio),
    }

