from __future__ import annotations

import pathlib
import unittest


def load_tests(
    loader: unittest.TestLoader,
    standard_tests: unittest.TestSuite,
    pattern: str | None,
) -> unittest.TestSuite:
    return loader.discover(str(pathlib.Path(__file__).parent), pattern or "test*.py")
