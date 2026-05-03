=== Scope Validation Report ===
Label: serde-feature-train-case-rule
Task: serde feature: Train-Case rename rule
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/serde-feature-train-case-rule
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        19491
  Reasoning tokens:    14293
  Navigation tokens:   5198 (26.7%)
  - Wasted reads:      1553 (29.9%)
  - Redundant reads:   3652 (70.3%)
  - Useful reads:      3645 (70.1%)
  - Overhead/no-path:  0 (0.0%)
  Files read:          50
  Files useful:        1
  Files wasted:        49

ORACLE (perfect Scope):
  Navigation tokens:   33
  Token savings:       5165 (99.4% reduction)
  Reduction factor:    157.52

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- serde_derive_internals/src/case.rs (3645 tokens, 3 reads)

TOP WASTED FILES:
- serde_derive_internals/src/attr.rs (73 tokens, 2 reads)
- .github/ISSUE_TEMPLATE/1-problem.md (34 tokens, 1 reads)
- .github/ISSUE_TEMPLATE/2-suggestion.md (34 tokens, 1 reads)
- .github/ISSUE_TEMPLATE/3-documentation.md (34 tokens, 1 reads)
- .github/ISSUE_TEMPLATE/4-other.md (33 tokens, 1 reads)
