=== Scope Spike V3 Report ===
Run: ripgrep-refactor-parse-human-size-cold
Task: ripgrep cold-start refactor: shorten verbose size-parser API
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/BurntSushi_ripgrep
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- crates/cli/src/human.rs
- crates/cli/src/lib.rs
- crates/core/flags/defs.rs

LEXICAL BASELINE:
  Recall@5:          66.7%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:125896

SCOPE RANKER:
  Recall@5:          66.7%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:145971

TOP SCOPE FILES:
- crates/cli/src/human.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.766)
- crates/core/flags/hiargs.rs (import-neighbor signal, git co-change signal; score=0.707)
- crates/printer/src/json.rs (import-neighbor signal, git co-change signal; score=0.651)
- crates/globset/src/glob.rs (import-neighbor signal, git co-change signal; score=0.629)
- crates/core/flags/defs.rs (import-neighbor signal, git co-change signal; score=0.628)
