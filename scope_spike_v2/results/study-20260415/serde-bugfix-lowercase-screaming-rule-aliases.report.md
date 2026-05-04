=== Scope Spike V2 Report ===
Run: serde-bugfix-lowercase-screaming-rule-aliases
Task: serde bugfix: lowercase screaming rename-rule aliases
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/serde-rs_serde
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- serde_derive/src/internals/case.rs
- test_suite/tests/test_macros.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:29811

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:76126

TOP SCOPE FILES:
- serde_derive/src/internals/case.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.710)
- test_suite/tests/test_macros.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.501)
- serde_core/src/de/impls.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.348)
- serde_core/src/de/mod.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.338)
- serde_derive/src/de/enum_.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.327)
