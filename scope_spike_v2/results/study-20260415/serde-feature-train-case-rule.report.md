=== Scope Spike V2 Report ===
Run: serde-feature-train-case-rule
Task: serde feature: Train-Case rename rule
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/serde-rs_serde
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- serde_derive/src/internals/case.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:29813

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:91025

TOP SCOPE FILES:
- serde_derive/src/internals/case.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.691)
- serde_core/src/de/mod.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.327)
- serde_core/src/de/ignored_any.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.325)
- serde_core/src/de/value.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.313)
- serde_derive/src/de/enum_.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.311)
