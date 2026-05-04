=== Scope Spike V2 Report ===
Run: serde-refactor-multiname-accessors
Task: serde refactor: rename MultiName accessors
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/serde-rs_serde
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- serde_derive/src/internals/name.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:62479

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:71285

TOP SCOPE FILES:
- serde_derive/src/internals/name.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.709)
- serde/src/private/de.rs (path/keyword match, import-neighbor signal, git co-change signal, sibling-file signal; score=0.505)
- serde_derive/src/de.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.473)
- serde/src/private/ser.rs (path/keyword match, import-neighbor signal, git co-change signal, sibling-file signal; score=0.438)
- serde_derive/src/de/enum_adjacently.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.432)
