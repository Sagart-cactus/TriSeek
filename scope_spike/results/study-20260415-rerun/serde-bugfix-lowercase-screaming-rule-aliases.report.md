=== Scope Validation Report ===
Label: serde-bugfix-lowercase-screaming-rule-aliases
Task: serde bugfix: lowercase screaming rename-rule aliases
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/serde-bugfix-lowercase-screaming-rule-aliases
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        27540
  Reasoning tokens:    23882
  Navigation tokens:   3658 (13.3%)
  - Wasted reads:      17 (0.5%)
  - Redundant reads:   3191 (87.2%)
  - Useful reads:      3304 (90.3%)
  - Overhead/no-path:  337 (9.2%)
  Files read:          7
  Files useful:        3
  Files wasted:        4

ORACLE (perfect Scope):
  Navigation tokens:   116
  Token savings:       3542 (96.8% reduction)
  Reduction factor:    31.53

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- serde_derive_internals/src/case.rs (1914 tokens, 3 reads)
- test_suite/tests/test_macros.rs (761 tokens, 3 reads)
- serde_derive/src/internals/case.rs (629 tokens, 3 reads)

TOP WASTED FILES:
- Cargo.toml (6 tokens, 2 reads)
- CONTRIBUTING.md (5 tokens, 1 reads)
- crates-io.md (4 tokens, 1 reads)
- README.md (2 tokens, 1 reads)
