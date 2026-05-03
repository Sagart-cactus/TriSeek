=== Scope Spike V3 Report ===
Run: serde-refactor-multiname-accessors-cold
Task: serde cold-start refactor: clearer serializer/deserializer name accessors
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/serde-rs_serde
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- serde_derive/src/de.rs
- serde_derive/src/de/enum_adjacently.rs
- serde_derive/src/de/enum_externally.rs
- serde_derive/src/de/struct_.rs
- serde_derive/src/de/tuple.rs
- serde_derive/src/de/unit.rs
- serde_derive/src/internals/check.rs
- serde_derive/src/internals/name.rs
- serde_derive/src/ser.rs

LEXICAL BASELINE:
  Recall@5:          22.2%
  Recall@10:         33.3%
  MRR:               0.250
  Full coverage@10:  False
  Top-10 read budget:55310

SCOPE RANKER:
  Recall@5:          11.1%
  Recall@10:         22.2%
  MRR:               0.200
  Full coverage@10:  False
  Top-10 read budget:52063

TOP SCOPE FILES:
- serde/README.md (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal, sibling-file signal; score=0.775)
- serde_core/src/de/mod.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.742)
- serde_derive/README.md (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.727)
- test_suite/tests/test_borrow.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.725)
- serde_derive/src/de.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.707)
