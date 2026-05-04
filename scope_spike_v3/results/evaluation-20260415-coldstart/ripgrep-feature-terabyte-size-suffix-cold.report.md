=== Scope Spike V3 Report ===
Run: ripgrep-feature-terabyte-size-suffix-cold
Task: ripgrep cold-start feature: next larger byte unit
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/BurntSushi_ripgrep
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- crates/cli/src/human.rs
- crates/core/flags/defs.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               0.333
  Full coverage@10:  True
  Top-10 read budget:142268

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               0.333
  Full coverage@10:  True
  Top-10 read budget:132311

TOP SCOPE FILES:
- crates/core/flags/doc/help.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.732)
- ci/test-complete (path/keyword match, git co-change signal; score=0.680)
- crates/cli/src/human.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.661)
- crates/core/flags/defs.rs (import-neighbor signal, git co-change signal; score=0.624)
- crates/ignore/src/walk.rs (import-neighbor signal, git co-change signal; score=0.560)
