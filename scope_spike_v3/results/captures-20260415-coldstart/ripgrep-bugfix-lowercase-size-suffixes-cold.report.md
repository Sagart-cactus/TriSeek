=== Scope Validation Report ===
Label: ripgrep-bugfix-lowercase-size-suffixes-cold
Task: ripgrep cold-start bugfix: lowercase unit spellings
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/ripgrep-bugfix-lowercase-size-suffixes-cold
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        6930
  Reasoning tokens:    4445
  Navigation tokens:   2485 (35.9%)
  - Wasted reads:      723 (29.1%)
  - Redundant reads:   1037 (41.7%)
  - Useful reads:      1044 (42.0%)
  - Overhead/no-path:  718 (28.9%)
  Files read:          96
  Files useful:        1
  Files wasted:        95

ORACLE (perfect Scope):
  Navigation tokens:   7
  Token savings:       2478 (99.7% reduction)
  Reduction factor:    355.0

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
