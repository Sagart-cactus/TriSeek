from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from context_pack_validation.report import write_reports

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RESULTS_ROOT = REPO_ROOT / "context_pack_validation" / "results"


def run_agent_smoke(*, results_root: Path | None = None, enable_agent_smoke: bool = False) -> dict[str, Any]:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")
    output_root = (results_root or DEFAULT_RESULTS_ROOT / f"agent-smoke-{timestamp}").resolve()
    if not enable_agent_smoke:
        output_root.mkdir(parents=True, exist_ok=True)
        payload = {
            "skipped": True,
            "reason": "agent smoke is optional; pass --enable-agent-smoke to run live-agent flows",
        }
        (output_root / "agent-smoke.json").write_text(json.dumps(payload, indent=2) + "\n")
        return {"output_root": str(output_root), **payload}

    runs = [
        {
            "id": "agent_smoke_placeholder",
            "group": "agent_smoke",
            "verdict": "neutral",
            "hit_at_1": False,
            "hit_at_4": False,
            "mrr": 0.0,
            "oracle_coverage": 0.0,
            "precision_at_pack": 0.0,
            "misleading_top1": False,
            "expansion_needed": True,
            "pack_tokens": 0,
            "baseline_tokens": 0,
            "token_reduction_ratio": 0.0,
            "pack_tool_calls": 0,
            "baseline_tool_calls": 0,
            "tool_call_reduction": 0,
            "pack_latency_ms": 0.0,
            "baseline_latency_ms": 0.0,
            "latency_ratio": 0.0,
            "cli_mcp_paths_match": False,
            "note": "Live-agent workflow adapter is intentionally separate from deterministic claims.",
        }
    ]
    aggregate = write_reports(output_root, runs)
    return {"output_root": str(output_root), "aggregate": aggregate, "runs": runs}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results-root", type=Path)
    parser.add_argument("--enable-agent-smoke", action="store_true")
    args = parser.parse_args()
    outcome = run_agent_smoke(
        results_root=args.results_root,
        enable_agent_smoke=args.enable_agent_smoke,
    )
    print(json.dumps(outcome, indent=2))


if __name__ == "__main__":
    main()
