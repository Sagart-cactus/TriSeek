# Search Reuse Validation Summary (search-reuse-20260423T104732Z)

- Total runs:                    12
- File false negatives:         0
- Search false negatives:       1
- Post-compact false negatives: 0
- File saved / eligible:        496 / 496
- Search saved / eligible:      0 / 679
- Combined saved / eligible:    496 / 1175
- Duplicate search hits:        0 / 1
- Combined reduction ratio:     42.2%
- PASS / FAIL:                  11 / 1

## Per-run
- ripgrep-bugfix-lowercase-size-suffixes: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- ripgrep-feature-terabyte-size-suffix: PASS  file_saved=202 search_saved=0 search_hits=0 combined_saved=202
- ripgrep-refactor-parse-human-size: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- serde-bugfix-lowercase-screaming-rule-aliases: PASS  file_saved=130 search_saved=0 search_hits=0 combined_saved=130
- serde-feature-train-case-rule: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- serde-refactor-multiname-accessors: FAIL  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- ripgrep-bugfix-lowercase-size-suffixes-cold: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- ripgrep-feature-terabyte-size-suffix-cold: PASS  file_saved=105 search_saved=0 search_hits=0 combined_saved=105
- ripgrep-refactor-parse-human-size-cold: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- serde-bugfix-lowercase-screaming-rule-aliases-cold: PASS  file_saved=59 search_saved=0 search_hits=0 combined_saved=59
- serde-feature-train-case-rule-cold: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0
- serde-refactor-multiname-accessors-cold: PASS  file_saved=0 search_saved=0 search_hits=0 combined_saved=0