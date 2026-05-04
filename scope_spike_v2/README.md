# Scope Spike V2

`scope_spike_v2` evaluates a real file-retrieval strategy against the six
captured study tasks from `scope_spike/results/study-20260415-rerun`.

Unlike the original `scope_spike`, this package does not use a perfect
hindsight oracle. It ranks files from the clean benchmark repos with:

- lexical retrieval,
- light import-graph propagation,
- git co-change propagation,
- test-file heuristics.

It reports retrieval quality against the edited files from the captured traces
and compares the full Scope-style ranking against a lexical-only baseline.

## Run

```bash
./scope_spike/.venv/bin/python -m scope_spike_v2.evaluate
```

## Default Inputs

- Manifest: `scope_spike/study_manifest.json`
- Ground truth: `scope_spike/results/study-20260415-rerun`

## Output

Timestamped reports are written under `scope_spike_v2/results/`.

