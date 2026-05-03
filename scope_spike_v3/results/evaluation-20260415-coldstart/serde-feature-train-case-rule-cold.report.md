=== Scope Spike V3 Report ===
Run: serde-feature-train-case-rule-cold
Task: serde cold-start feature: dashed title-case rename style
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
  Top-10 read budget:19605

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:48115

TOP SCOPE FILES:
- serde_derive/src/internals/case.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.704)
- serde_derive/src/de/enum_.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.552)
- test_suite/tests/test_macros.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.547)
- serde_derive/src/de/enum_untagged.rs (path/keyword match, import-neighbor signal, git co-change signal; score=0.514)
- test_suite/tests/test_identifier.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.514)
