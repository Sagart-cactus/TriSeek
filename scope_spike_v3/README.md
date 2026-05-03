# Scope Spike V3

`scope_spike_v3` redesigns the benchmark itself around colder, less lexical
task prompts.

The goal is not to change the retrieval algorithm from `scope_spike_v2`. The
goal is to make the task descriptions more like real ambiguous coding requests
where exact symbol names are not handed to the agent up front.

## Workflow

1. Capture a fresh 6-run study using the cold-start manifest.
2. Evaluate lexical-only retrieval vs the Scope-style reranker.
3. Run the realistic Memo simulation against the fresh traces.

## Commands

```bash
./scope_spike/.venv/bin/python -m scope_spike_v3.capture_study
./scope_spike/.venv/bin/python -m scope_spike_v3.evaluate
./scope_spike/.venv/bin/python -m memo_spike_v3.study
```

