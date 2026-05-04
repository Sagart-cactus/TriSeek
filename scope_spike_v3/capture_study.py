from __future__ import annotations

import argparse
import json
from pathlib import Path

from scope_spike.study import run_study

DEFAULT_MANIFEST = Path("scope_spike_v3/study_manifest.json")
DEFAULT_RESULTS_ROOT = Path("scope_spike_v3/results/captures")


def main() -> None:
    parser = argparse.ArgumentParser(description="Capture the cold-start v3 study traces.")
    parser.add_argument("--manifest", default=str(DEFAULT_MANIFEST), help="Cold-start study manifest.")
    parser.add_argument(
        "--results-root",
        help="Directory for captured run reports. Defaults to scope_spike_v3/results/captures.",
    )
    parser.add_argument("--model", help="Optional Claude model override.")
    args = parser.parse_args()
    result = run_study(
        manifest_path=args.manifest,
        results_root=args.results_root or str(DEFAULT_RESULTS_ROOT),
        model=args.model,
    )
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

