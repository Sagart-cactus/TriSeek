=== Scope Validation Report ===
Label: ripgrep-feature-terabyte-size-suffix
Task: ripgrep feature: terabyte size suffix
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/ripgrep-feature-terabyte-size-suffix
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        20402
  Reasoning tokens:    12404
  Navigation tokens:   7998 (39.2%)
  - Wasted reads:      2322 (29.0%)
  - Redundant reads:   1457 (18.2%)
  - Useful reads:      1480 (18.5%)
  - Overhead/no-path:  4196 (52.5%)
  Files read:          89
  Files useful:        2
  Files wasted:        87

ORACLE (perfect Scope):
  Navigation tokens:   42
  Token savings:       7956 (99.5% reduction)
  Reduction factor:    190.43

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- crates/cli/src/human.rs (1051 tokens, 3 reads)
- crates/core/flags/defs.rs (429 tokens, 5 reads)

TOP WASTED FILES:
- crates/core/flags/complete/rg.zsh (49 tokens, 2 reads)
- crates/core/flags/doc/help.rs (43 tokens, 2 reads)
- crates/core/flags/complete/encodings.sh (38 tokens, 1 reads)
- crates/core/flags/complete/powershell.rs (38 tokens, 1 reads)
- crates/core/flags/complete/prelude.fish (38 tokens, 1 reads)
