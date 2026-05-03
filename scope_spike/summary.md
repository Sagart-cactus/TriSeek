# Scope Spike Status

## Current State

- Standalone Python harness implemented under `scope_spike/`
- Real Claude JSONL parsing works against stored traces
- Unit tests pass
- Fresh 6-run study is currently blocked by local Claude CLI authentication

## Smoke Validation

- `scope_spike/results/smoke/codexbar-smoke.report.json`
  - verdict: `FAIL`
  - elimination ratio: `1.7%`
  - note: real edit-heavy trace with minimal navigation waste
- `scope_spike/results/smoke/torvalds-grep-smoke.report.json`
  - verdict: `PASS`
  - elimination ratio: `100.0%`
  - note: search-only trace with no edited files, useful for parser validation but not for the final Scope decision

## Study Attempt

- `scope_spike/results/study-attempt-20260414/summary.md`
- status: `blocked`
- blocker: local `claude -p` returns a 401 authentication error before the first benchmark run

## Decision Status

No trustworthy Scope build/no-build verdict exists yet from the requested 6 fresh runs.
The harness is ready; the remaining step is to re-authenticate Claude Code and rerun:

```bash
./scope_spike/.venv/bin/python -m scope_spike.study
```
