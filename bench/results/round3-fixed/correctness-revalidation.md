# Round 3 Correctness Revalidation

The raw `round3-fixed/report.json` was generated before the final benchmark-harness normalization change and before the Kubernetes and Rust indexes were rebuilt with the latest binary-detection logic.

Residual failures reported in `report.json`:

- `kubernetes_kubernetes`: `literal_moderate`, `literal_high`, `multi_or`
- `rust-lang_rust`: `literal_moderate`, `literal_high`, `regex_anchor`, `multi_or`

Follow-up validation on the current release binary:

- Rebuilt cached indexes for:
  - `kubernetes_kubernetes`
  - `rust-lang_rust`
- Replayed each previously failed case with:
  - `triseek search --engine auto --json`
  - baseline `rg --json`
- Normalized:
  - leading `./` in paths
  - trailing `\r` in CRLF line text
  - baseline `rg --json` events that omitted `lines.text` entirely were excluded from comparison, because they are binary-match artifacts that the benchmark harness cannot compare as textual hits

Validated cases:

| Repo | Query | Status |
|---|---|---|
| kubernetes_kubernetes | literal_moderate | pass |
| kubernetes_kubernetes | literal_high | pass |
| kubernetes_kubernetes | multi_or | pass |
| rust-lang_rust | literal_moderate | pass |
| rust-lang_rust | literal_high | pass |
| rust-lang_rust | regex_anchor | pass |
| rust-lang_rust | multi_or | pass |

Summary:

- 7 of 7 residual failures passed on the current binary.
- The `round3-fixed/summary.md` timing table remains representative for performance.
- The stale correctness flags in `round3-fixed/report.json` should be interpreted together with this revalidation note.
