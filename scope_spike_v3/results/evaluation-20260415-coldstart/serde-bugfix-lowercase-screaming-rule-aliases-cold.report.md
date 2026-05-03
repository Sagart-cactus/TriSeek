=== Scope Spike V3 Report ===
Run: serde-bugfix-lowercase-screaming-rule-aliases-cold
Task: serde cold-start bugfix: lowercase screaming rename aliases
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/serde-rs_serde
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- serde_derive/src/internals/case.rs
- test_suite/tests/test_macros.rs

LEXICAL BASELINE:
  Recall@5:          50.0%
  Recall@10:         50.0%
  MRR:               1.000
  Full coverage@10:  False
  Top-10 read budget:49390

SCOPE RANKER:
  Recall@5:          50.0%
  Recall@10:         50.0%
  MRR:               1.000
  Full coverage@10:  False
  Top-10 read budget:49563

TOP SCOPE FILES:
- serde_derive/src/internals/case.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.720)
- test_suite/tests/test_identifier.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.504)
- serde_core/Cargo.toml (path/keyword match, git co-change signal; score=0.492)
- serde_core/src/de/impls.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.486)
- serde_derive/src/de/struct_.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.467)
