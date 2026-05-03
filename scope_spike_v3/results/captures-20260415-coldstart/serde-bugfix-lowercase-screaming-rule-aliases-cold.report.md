=== Scope Validation Report ===
Label: serde-bugfix-lowercase-screaming-rule-aliases-cold
Task: serde cold-start bugfix: lowercase screaming rename aliases
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/serde-bugfix-lowercase-screaming-rule-aliases-cold
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        25997
  Reasoning tokens:    19320
  Navigation tokens:   6677 (25.7%)
  - Wasted reads:      1434 (21.5%)
  - Redundant reads:   4364 (65.4%)
  - Useful reads:      4245 (63.6%)
  - Overhead/no-path:  998 (14.9%)
  Files read:          44
  Files useful:        3
  Files wasted:        41

ORACLE (perfect Scope):
  Navigation tokens:   24
  Token savings:       6653 (99.6% reduction)
  Reduction factor:    278.21

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- serde_derive/src/internals/case.rs (1708 tokens, 2 reads)
- serde_derive_internals/src/case.rs (1708 tokens, 2 reads)
- test_suite/tests/test_macros.rs (829 tokens, 3 reads)

TOP WASTED FILES:
- Cargo.toml (146 tokens, 3 reads)
- serde_core/src/ser/impls.rs (42 tokens, 1 reads)
- serde_derive_internals/src/attr.rs (42 tokens, 1 reads)
- serde_derive_internals/src/ctxt.rs (42 tokens, 1 reads)
- serde_derive_internals/src/receiver.rs (42 tokens, 1 reads)
