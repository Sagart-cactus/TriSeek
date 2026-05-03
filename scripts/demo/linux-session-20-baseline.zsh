#!/usr/bin/env zsh
set -euo pipefail

rg --json --line-number --color never --no-heading --fixed-strings --max-count 50 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 200 also . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 500 License . >/dev/null
rg --json --line-number --color never --no-heading --max-count 200 '\balso\b' . >/dev/null
rg --json --line-number --color never --no-heading --max-count 200 'License|Identifier|SPDX' . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings -g '*.c' --max-count 100 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 50 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 200 also . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 500 License . >/dev/null
rg --json --line-number --color never --no-heading --max-count 200 '\balso\b' . >/dev/null
rg --json --line-number --color never --no-heading --max-count 200 'License|Identifier|SPDX' . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings -g '*.c' --max-count 100 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 50 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 200 also . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 500 License . >/dev/null
rg --json --line-number --color never --no-heading --max-count 200 '\balso\b' . >/dev/null
rg --json --line-number --color never --no-heading --max-count 200 'License|Identifier|SPDX' . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings -g '*.c' --max-count 100 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 50 AEGIS_BLOCK_SIZE . >/dev/null
rg --json --line-number --color never --no-heading --fixed-strings --max-count 200 also . >/dev/null
