=== Scope Spike V3 Report ===
Run: ripgrep-bugfix-lowercase-size-suffixes-cold
Task: ripgrep cold-start bugfix: lowercase unit spellings
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/BurntSushi_ripgrep
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- crates/cli/src/human.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:141146

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               0.500
  Full coverage@10:  True
  Top-10 read budget:127233

TOP SCOPE FILES:
- crates/regex/src/matcher.rs (import-neighbor signal, git co-change signal, sibling-file signal; score=0.810)
- crates/cli/src/human.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.764)
- crates/core/flags/defs.rs (import-neighbor signal, git co-change signal; score=0.718)
- crates/cli/src/lib.rs (import-neighbor signal, git co-change signal; score=0.717)
- GUIDE.md (git co-change signal; score=0.657)
