from __future__ import annotations

import math

TOKENIZER_NAME = "cl100k_base"

try:
    import tiktoken
except Exception:  # pragma: no cover - exercised indirectly in environments without tiktoken
    tiktoken = None
    TOKENIZER_NAME = "len4-fallback"
    _ENCODER = None
else:
    _ENCODER = tiktoken.get_encoding("cl100k_base")


def count_tokens(text: str) -> int:
    if not text:
        return 0
    if _ENCODER is not None:
        return len(_ENCODER.encode(text))
    return max(1, math.ceil(len(text) / 4))


def tokenizer_metadata() -> dict[str, object]:
    return {
        "name": TOKENIZER_NAME,
        "exact": _ENCODER is not None,
    }
