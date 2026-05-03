=== Scope Validation Report ===
Label: codexbar-smoke
Task: smoke validation on a real edit-heavy Claude trace
Repository: /Users/trivedi/Documents/Projects/CodexBar
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        2913
  Reasoning tokens:    2738
  Navigation tokens:   175 (6.0%)
  - Wasted reads:      0 (0.0%)
  - Redundant reads:   0 (0.0%)
  - Useful reads:      172 (98.3%)
  - Overhead/no-path:  3 (1.7%)
  Files read:          1
  Files useful:        1
  Files wasted:        0

ORACLE (perfect Scope):
  Navigation tokens:   172
  Token savings:       3 (1.7% reduction)
  Reduction factor:    1.02

VERDICT: FAIL
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- .github/workflows/ci.yml (172 tokens, 1 reads)
