=== Scope Spike V2 Report ===
Run: ripgrep-bugfix-lowercase-size-suffixes
Task: ripgrep bugfix: lowercase size suffixes
Repository: /Users/trivedi/Documents/Projects/Triseek-bench/repos/BurntSushi_ripgrep
Tokenizer: cl100k_base

GROUND TRUTH FILES:
- crates/cli/src/human.rs

LEXICAL BASELINE:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:108514

SCOPE RANKER:
  Recall@5:          100.0%
  Recall@10:         100.0%
  MRR:               1.000
  Full coverage@10:  True
  Top-10 read budget:110386

TOP SCOPE FILES:
- crates/cli/src/human.rs (exact symbol match, path/keyword match, import-neighbor signal, git co-change signal; score=0.757)
- crates/regex/src/error.rs (path/keyword match, import-neighbor signal, git co-change signal, sibling-file signal; score=0.579)
- crates/pcre2/src/error.rs (path/keyword match, import-neighbor signal, git co-change signal, sibling-file signal; score=0.543)
- crates/cli/src/lib.rs (exact symbol match, import-neighbor signal, git co-change signal; score=0.522)
- crates/core/flags/defs.rs (import-neighbor signal, git co-change signal; score=0.457)
