=== Scope Validation Report ===
Label: serde-refactor-multiname-accessors
Task: serde refactor: rename MultiName accessors
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/serde-refactor-multiname-accessors
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        9445
  Reasoning tokens:    8295
  Navigation tokens:   1150 (12.2%)
  - Wasted reads:      693 (60.3%)
  - Redundant reads:   912 (79.3%)
  - Useful reads:      324 (28.2%)
  - Overhead/no-path:  133 (11.6%)
  Files read:          15
  Files useful:        2
  Files wasted:        13

ORACLE (perfect Scope):
  Navigation tokens:   16
  Token savings:       1134 (98.6% reduction)
  Reduction factor:    71.88

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- serde_derive/src/internals/name.rs (162 tokens, 3 reads)
- serde_derive_internals/src/name.rs (162 tokens, 3 reads)

TOP WASTED FILES:
- serde_derive/src/ser.rs (312 tokens, 2 reads)
- serde_derive/src/de/struct_.rs (105 tokens, 2 reads)
- serde_derive/src/de/tuple.rs (77 tokens, 2 reads)
- serde_derive/src/de/enum_adjacently.rs (37 tokens, 2 reads)
- serde_derive/src/de/enum_externally.rs (37 tokens, 2 reads)
