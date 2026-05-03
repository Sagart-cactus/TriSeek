=== Scope Validation Report ===
Label: serde-feature-train-case-rule-cold
Task: serde cold-start feature: dashed title-case rename style
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/serde-feature-train-case-rule-cold
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        28834
  Reasoning tokens:    24164
  Navigation tokens:   4670 (16.2%)
  - Wasted reads:      408 (8.7%)
  - Redundant reads:   1699 (36.4%)
  - Useful reads:      4262 (91.3%)
  - Overhead/no-path:  0 (0.0%)
  Files read:          4
  Files useful:        3
  Files wasted:        1

ORACLE (perfect Scope):
  Navigation tokens:   2563
  Token savings:       2107 (45.1% reduction)
  Reduction factor:    1.82

VERDICT: FAIL
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- serde_derive/src/internals/case.rs (1869 tokens, 2 reads)
- serde_derive_internals/src/case.rs (1699 tokens, 1 reads)
- test_suite/tests/test_macros.rs (694 tokens, 1 reads)

TOP WASTED FILES:
- serde_derive/src/internals/attr.rs (408 tokens, 1 reads)
