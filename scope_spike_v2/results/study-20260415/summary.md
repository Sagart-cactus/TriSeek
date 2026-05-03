# Scope Spike V2 Summary (study-20260415)

- Total runs: 6
- Lexical avg Recall@5: 100.0%
- Scope avg Recall@5: 100.0%
- Lexical avg Recall@10: 100.0%
- Scope avg Recall@10: 100.0%
- Lexical avg MRR: 1.000
- Scope avg MRR: 1.000
- Lexical full coverage@10 runs: 6
- Scope full coverage@10 runs: 6
- Scope avg top-5 read budget: 61466 tokens
- Scope avg top-10 read budget: 94986 tokens
- Incremental verdict over lexical baseline: FAIL

## Runs
- ripgrep-bugfix-lowercase-size-suffixes: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- ripgrep-feature-terabyte-size-suffix: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- ripgrep-refactor-parse-human-size: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- serde-bugfix-lowercase-screaming-rule-aliases: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- serde-feature-train-case-rule: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- serde-refactor-multiname-accessors: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
