=== Scope Spike V2 Report ===
Run: ripgrep-refactor-parse-human-size
Task: ripgrep refactor: rename parse_human_readable_size
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/BurntSushi_ripgrep
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- crates/cli/src/human.rs
- crates/cli/src/lib.rs
- crates/core/flags/defs.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:115018

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:110855

TOP SCOPE FILES:
- crates/cli/src/human.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.753)
- crates/core/flags/parse.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.506)
- crates/core/flags/defs.rs (exact symbol match, import-neighbor signal, git co-change signal; score=0.497)
- crates/cli/src/lib.rs (exact symbol match, import-neighbor signal, git co-change signal; score=0.492)
- crates/core/flags/hiargs.rs (import-neighbor signal, git co-change signal; score=0.432)
