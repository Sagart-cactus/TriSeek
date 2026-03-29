# Decision Memo

TriSeek should not replace the current shell-based search path as the default runtime yet. The final benchmark run showed acceptable correctness on the covered workloads after revalidation, but the indexed engine still regressed latency, repeated-session behavior, and memory footprint across every measured repo category.

## Inputs

- Benchmark summary: `bench/results/final-run/summary.md`
- Raw timing artifacts: `bench/results/final-run/report.json` and `bench/results/final-run/report.csv`
- Correctness revalidation: `bench/results/final-run/correctness-revalidation.md`
- Full repo measurements: `bench/results/prepared_repos_full.json`

## Recommendation Table

| Repo Category | Cold Start Winner | Repeated Session Winner | CPU Winner | Memory Winner | Recommended Default |
|---|---|---|---|---|---|
| Small | `rg` / shell tools | `rg` / shell tools | `rg` / shell tools | roughly tied | `rg` for content, `fd` or `rg --files` for paths |
| Medium | `rg` / shell tools | `rg` / shell tools | mixed, slight TriSeek edge on some cases | `rg` / shell tools | `rg` for content, `fd` or `rg --files` for paths |
| Large | `rg` / shell tools | `rg` / shell tools | mixed, roughly tied | `rg` / shell tools by a large margin | `rg` for content, `fd` or `rg --files` for paths |
| Very Large | not benchmarked | not benchmarked | not benchmarked | not benchmarked | do not activate indexed search until a real very-large corpus is measured |

## Decision Criteria

- Correctness: acceptable for the benchmarked workload families after the final revalidation pass.
- Cold-start latency: fails the bar. TriSeek was slower in 38 of 39 timed single-query cases.
- Repeated-session speedup: fails the bar. No session benchmark beat the shell baseline.
- Build/update amortization: fails the bar on medium and large repos. The large-repo index took 68.3 s to build and 9.9 s to update.
- Memory/operational cost: fails the bar. The indexed path had materially higher resident-set usage and adds index lifecycle complexity.

## Outcome

- Integration decision: do not integrate the indexed engine as the default search path.
- Activation policy: keep the existing shell-based routing for all measured categories.
- Fallback policy:
  - content search: `rg`
  - suffix/exact filename search: `fd`
  - full path listing or substring search: `rg --files`-based flow
  - unsupported or degenerate queries: direct shell fallback
- Indexed engine status: keep available only behind an explicit opt-in or developer flag while it is being optimized.

## What Would Need to Improve

- Stronger pruning so high-match and path-heavy queries do not fan out into massive verification sets.
- Lower per-query memory pressure, especially on large indexes.
- Better literal extraction for regex planning and better handling of path-only workloads.
- Query/session caching that actually amortizes repeated searches.
- A validated benchmark on a real `very_large` corpus before claiming any threshold where indexed search should activate automatically.
