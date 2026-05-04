=== Scope Validation Report ===
Label: serde-refactor-multiname-accessors-cold
Task: serde cold-start refactor: clearer serializer/deserializer name accessors
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/serde-refactor-multiname-accessors-cold
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        57591
  Reasoning tokens:    50155
  Navigation tokens:   7436 (12.9%)
  - Wasted reads:      3796 (51.0%)
  - Redundant reads:   3606 (48.5%)
  - Useful reads:      3640 (49.0%)
  - Overhead/no-path:  0 (0.0%)
  Files read:          113
  Files useful:        11
  Files wasted:        102

ORACLE (perfect Scope):
  Navigation tokens:   388
  Token savings:       7048 (94.8% reduction)
  Reduction factor:    19.16

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- serde_derive/src/internals/name.rs (820 tokens, 3 reads)
- serde_derive/src/ser.rs (771 tokens, 3 reads)
- serde_derive_internals/src/name.rs (720 tokens, 2 reads)
- serde_derive/src/de/struct_.rs (304 tokens, 3 reads)
- serde_derive/src/de/tuple.rs (245 tokens, 3 reads)

TOP WASTED FILES:
- serde_derive_internals/Cargo.toml (72 tokens, 2 reads)
- serde_derive_internals/LICENSE-APACHE (72 tokens, 2 reads)
- serde_derive_internals/src/attr.rs (72 tokens, 2 reads)
- serde_derive_internals/src/receiver.rs (72 tokens, 2 reads)
- serde_derive_internals/src/respan.rs (72 tokens, 2 reads)
