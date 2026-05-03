# Scope Spike Summary (captures-20260415-coldstart)

- Total runs: 6
- Average elimination ratio: 89.0%
- Median elimination ratio: 97.5%
- Range: 45.1% to 99.7%
- PASS-count (>80% elimination): 5
- Verdict: PASS

## Runs
- ripgrep-bugfix-lowercase-size-suffixes-cold: PASS (99.7% elimination, 355.0x reduction)
- ripgrep-feature-terabyte-size-suffix-cold: PASS (98.7% elimination, 78.68x reduction)
- ripgrep-refactor-parse-human-size-cold: PASS (96.3% elimination, 26.73x reduction)
- serde-bugfix-lowercase-screaming-rule-aliases-cold: PASS (99.6% elimination, 278.21x reduction)
- serde-feature-train-case-rule-cold: FAIL (45.1% elimination, 1.82x reduction)
- serde-refactor-multiname-accessors-cold: PASS (94.8% elimination, 19.16x reduction)
