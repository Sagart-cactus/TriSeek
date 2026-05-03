# Scope Spike V3 Summary (evaluation-20260415-coldstart)

- Total runs: 6
- Lexical avg Recall@5: 73.1%
- Scope avg Recall@5: 71.3%
- Lexical avg Recall@10: 80.6%
- Scope avg Recall@10: 78.7%
- Lexical avg MRR: 0.764
- Scope avg MRR: 0.672
- Lexical full coverage@10 runs: 4
- Scope full coverage@10 runs: 4
- Scope avg top-5 read budget: 56898 tokens
- Scope avg top-10 read budget: 92543 tokens
- Incremental verdict over lexical baseline: FAIL

## Runs
- ripgrep-bugfix-lowercase-size-suffixes-cold: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- ripgrep-feature-terabyte-size-suffix-cold: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- ripgrep-refactor-parse-human-size-cold: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- serde-bugfix-lowercase-screaming-rule-aliases-cold: lexical R@10 50.0%, scope R@10 50.0%, scope full@10=False
- serde-feature-train-case-rule-cold: lexical R@10 100.0%, scope R@10 100.0%, scope full@10=True
- serde-refactor-multiname-accessors-cold: lexical R@10 33.3%, scope R@10 22.2%, scope full@10=False
