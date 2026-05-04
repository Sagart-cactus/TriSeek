=== Scope Validation Report ===
Label: ripgrep-feature-terabyte-size-suffix-cold
Task: ripgrep cold-start feature: next larger byte unit
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/ripgrep-feature-terabyte-size-suffix-cold
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        13727
  Reasoning tokens:    10265
  Navigation tokens:   3462 (25.2%)
  - Wasted reads:      1359 (39.3%)
  - Redundant reads:   1374 (39.7%)
  - Useful reads:      1314 (38.0%)
  - Overhead/no-path:  789 (22.8%)
  Files read:          54
  Files useful:        2
  Files wasted:        52

ORACLE (perfect Scope):
  Navigation tokens:   44
  Token savings:       3418 (98.7% reduction)
  Reduction factor:    78.68

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- crates/cli/src/human.rs (1044 tokens, 2 reads)
- crates/core/flags/defs.rs (270 tokens, 4 reads)

TOP WASTED FILES:
- GUIDE.md (58 tokens, 3 reads)
- crates/core/flags/complete/rg.zsh (51 tokens, 2 reads)
- crates/core/flags/complete/zsh.rs (49 tokens, 2 reads)
- crates/searcher/src/line_buffer.rs (47 tokens, 2 reads)
- crates/searcher/src/searcher/mod.rs (47 tokens, 2 reads)
