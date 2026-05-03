=== Scope Validation Report ===
Label: ripgrep-refactor-parse-human-size
Task: ripgrep refactor: rename parse_human_readable_size
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/ripgrep-refactor-parse-human-size
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        9093
  Reasoning tokens:    6097
  Navigation tokens:   2996 (32.9%)
  - Wasted reads:      1307 (43.6%)
  - Redundant reads:   736 (24.6%)
  - Useful reads:      1038 (34.6%)
  - Overhead/no-path:  651 (21.7%)
  Files read:          42
  Files useful:        3
  Files wasted:        39

ORACLE (perfect Scope):
  Navigation tokens:   302
  Token savings:       2694 (89.9% reduction)
  Reduction factor:    9.92

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- crates/cli/src/human.rs (734 tokens, 2 reads)
- crates/cli/src/lib.rs (241 tokens, 2 reads)
- crates/core/flags/defs.rs (63 tokens, 2 reads)

TOP WASTED FILES:
- crates/ignore/tests/gitignore_matched_path_or_any_parents_tests.rs (39 tokens, 1 reads)
- crates/core/flags/complete/powershell.rs (36 tokens, 1 reads)
- crates/globset/benches/bench.rs (36 tokens, 1 reads)
- crates/ignore/tests/gitignore_skip_bom.rs (36 tokens, 1 reads)
- crates/core/flags/complete/fish.rs (35 tokens, 1 reads)
