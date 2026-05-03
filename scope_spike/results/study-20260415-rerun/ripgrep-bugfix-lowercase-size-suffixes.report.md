=== Scope Validation Report ===
Label: ripgrep-bugfix-lowercase-size-suffixes
Task: ripgrep bugfix: lowercase size suffixes
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/ripgrep-bugfix-lowercase-size-suffixes
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        5531
  Reasoning tokens:    3333
  Navigation tokens:   2198 (39.7%)
  - Wasted reads:      747 (34.0%)
  - Redundant reads:   1037 (47.2%)
  - Useful reads:      1044 (47.5%)
  - Overhead/no-path:  407 (18.5%)
  Files read:          100
  Files useful:        1
  Files wasted:        99

ORACLE (perfect Scope):
  Navigation tokens:   7
  Token savings:       2191 (99.7% reduction)
  Reduction factor:    314.0

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- crates/cli/src/human.rs (1044 tokens, 2 reads)

TOP WASTED FILES:
- crates/ignore/tests/gitignore_matched_path_or_any_parents_tests.rs (14 tokens, 1 reads)
- crates/core/flags/complete/powershell.rs (11 tokens, 1 reads)
- crates/globset/benches/bench.rs (11 tokens, 1 reads)
- crates/ignore/tests/gitignore_skip_bom.rs (11 tokens, 1 reads)
- crates/printer/src/hyperlink/aliases.rs (11 tokens, 1 reads)
