from __future__ import annotations

from pathlib import Path

import pytest


@pytest.fixture
def path(tmp_path: Path) -> Path:
    """Where a test's database goes: a temporary directory of its own."""
    return tmp_path / "app.darudb"
