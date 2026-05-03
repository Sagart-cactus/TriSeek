# Scope Spike

This directory contains a standalone Python spike for deciding whether a Scope-like
file-ranking tool is worth building for TriSeek.

It does not integrate with the Rust codebase. It does four things:

1. parses real Claude Code JSONL session logs,
2. estimates navigation-token waste,
3. computes a perfect-hindsight oracle,
4. aggregates repeated study runs into a verdict.

## Files

- `capture.py`: Claude session parsing and fresh-capture helpers
- `analyze.py`: navigation classification and token accounting
- `oracle.py`: perfect-hindsight file recommendation
- `report.py`: human-readable and JSON reporting
- `spike_runner.py`: analyze one trace file
- `study.py`: run or aggregate the 6-run study
- `study_manifest.json`: reproducible run matrix for the study

## Install

```bash
python3 -m venv scope_spike/.venv
./scope_spike/.venv/bin/python -m pip install -r scope_spike/requirements.txt
```

If `tiktoken` is missing, the spike falls back to a rough `len(text)/4` estimate.
That keeps local tests runnable, but the intended study uses `cl100k_base`.

## Analyze One Trace

```bash
./scope_spike/.venv/bin/python -m scope_spike.spike_runner \
  --trace ~/.claude/projects/<project>/<session>.jsonl \
  --task "ripgrep bugfix run"
```

## Run The Study

```bash
./scope_spike/.venv/bin/python -m scope_spike.study
```

The study runner copies each target repo into `scope_spike/workdirs/`, invokes
`claude -p` in that disposable copy, then writes reports under `scope_spike/results/`.

## Current Limitation

Fresh captures depend on a working local `claude` CLI session. If `claude -p`
returns a 401 authentication error, the capture stage stops and writes a
capture error note instead of inventing results.
