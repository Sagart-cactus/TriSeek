# Full-Rerun Correctness Revalidation

The raw `report.json` from `rerun-2026-04-02-all/` records one failing case:

- `kubernetes_kubernetes`: `regex_weak`

Revalidation method:

- Reran `triseek search --engine auto --kind regex --json '[A-Za-z_][A-Za-z0-9_]{10,}'`
- Compared against `rg --json --line-number --color never --no-heading '[A-Za-z_][A-Za-z0-9_]{10,}' .`
- Normalized leading `./` path prefixes and CRLF line endings before diffing

Findings:

- TriSeek returned all non-binary textual hits that the baseline returned.
- The remaining baseline-only matches all came from one vendored protobuf payload:
  - `vendor/sigs.k8s.io/kustomize/kyaml/openapi/kubernetesapi/v1_21_2/swagger.pb`
- In the uncapped comparison, `rg` emitted 2,547 additional line records from that protobuf blob, while TriSeek treated it as binary-like content and skipped it.
- The timed benchmark result is unchanged: TriSeek still won the workload on p50 latency at `650.310 ms` versus `1213.794 ms`.

Conclusion:

- The fresh full rerun has no broad correctness regression across the benchmark set.
- The one remaining raw mismatch is a binary/text classification disagreement on vendored protobuf content, not a general regex-search failure.
