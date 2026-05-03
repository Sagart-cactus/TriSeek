=== Scope Spike V2 Report ===
Run: ripgrep-feature-terabyte-size-suffix
Task: ripgrep feature: terabyte size suffix
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/BurntSushi_ripgrep
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- crates/cli/src/human.rs
- crates/core/flags/defs.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:133956

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:110236

TOP SCOPE FILES:
- crates/cli/src/human.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.744)
- crates/core/flags/doc/help.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.509)
- crates/core/flags/defs.rs (import-neighbor signal, git co-change signal; score=0.489)
- crates/core/flags/hiargs.rs (import-neighbor signal, git co-change signal; score=0.427)
- crates/printer/src/json.rs (import-neighbor signal, git co-change signal; score=0.411)
