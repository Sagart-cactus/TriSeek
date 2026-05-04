# Memo Spike V2

`memo_spike_v2` evaluates a realistic session-aware file cache against the
captured study traces from `scope_spike/results/study-20260415-rerun`.

Unlike the original Scope oracle, this simulation only gives Memo credit for:

- repeated reads of already-cached files,
- post-edit rereads where a diff would suffice,
- searches against files already cached in-session.

It does not erase the first read, broad search overhead, or uncached browsing.

## Run

```bash
./scope_spike/.venv/bin/python -m memo_spike_v2.study
```

## Output

Timestamped reports are written under `memo_spike_v2/results/`.

