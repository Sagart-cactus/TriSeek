=== Scope Validation Report ===
Label: ripgrep-refactor-parse-human-size-cold
Task: ripgrep cold-start refactor: shorten verbose size-parser API
Repository: /Users/trivedi/Documents/Projects/TriSeek/scope_spike/workdirs/ripgrep-refactor-parse-human-size-cold
Tokenizer: cl100k_base

ACTUAL SESSION:
  Total tokens:        9377
  Reasoning tokens:    6651
  Navigation tokens:   2726 (29.1%)
  - Wasted reads:      24 (0.9%)
  - Redundant reads:   1923 (70.5%)
  - Useful reads:      2025 (74.3%)
  - Overhead/no-path:  677 (24.8%)
  Files read:          11
  Files useful:        3
  Files wasted:        8

ORACLE (perfect Scope):
  Navigation tokens:   102
  Token savings:       2624 (96.3% reduction)
  Reduction factor:    26.73

VERDICT: PASS
  >5x reduction  -> PASS
  3-5x reduction -> MARGINAL
  <3x reduction  -> FAIL

TOP USEFUL FILES:
- crates/cli/src/human.rs (1537 tokens, 3 reads)
- crates/cli/src/lib.rs (396 tokens, 3 reads)
- crates/core/flags/defs.rs (92 tokens, 2 reads)

TOP WASTED FILES:
- RELEASE-CHECKLIST.md (5 tokens, 1 reads)
- rustfmt.toml (4 tokens, 1 reads)
- CHANGELOG.md (3 tokens, 1 reads)
- Cargo.toml (3 tokens, 1 reads)
- GUIDE.md (3 tokens, 1 reads)
