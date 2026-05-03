=== Scope Validation Report ===
Label: torvalds-grep-smoke
Task: smoke validation on stored torvalds_linux grep trace
Repository: /Users/trivedi/Documents/Projects/TriSeek-bench/repos/torvalds_linux
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        176
  Reasoning tokens:    161
  Navigation tokens:   15 (8.5%)
  - Wasted reads:      15 (100.0%)
  - Redundant reads:   0 (0.0%)
  - Useful reads:      0 (0.0%)
  - Overhead/no-path:  0 (0.0%)
  Files read:          1
  Files useful:        0
  Files wasted:        1

ORACLE (perfect Scope):
  Navigation tokens:   0
  Token savings:       15 (100.0% reduction)
  Reduction factor:    n/a

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP WASTED FILES:
- crypto/aegis.h (15 tokens, 1 reads)
