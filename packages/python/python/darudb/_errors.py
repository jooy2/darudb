"""The one error the package raises, with the engine's code beside its message.

The native module imports this module by name to raise the error, so it
imports nothing of the package's own.
"""

from __future__ import annotations

__all__ = ["DaruError"]


class DaruError(Exception):
    """A failure of the database, with one of the engine's error codes.

    ``code`` is stable, such as ``"NOT_FOUND"`` or ``"DUPLICATE_KEY"``, and
    is what a program tests; ``message`` says what happened, for a person.
    """

    code: str
    message: str

    def __init__(self, code: str, message: str) -> None:
        super().__init__(code, message)
        self.code = code
        self.message = message

    def __str__(self) -> str:
        return f"{self.code}: {self.message}"

    def __repr__(self) -> str:
        return f"DaruError({self.code!r}, {self.message!r})"


# Shown as `darudb.DaruError`, where a program imports it from.
DaruError.__module__ = "darudb"


def invalid(message: str) -> DaruError:
    """An ``INVALID_ARGUMENT`` error."""
    return DaruError("INVALID_ARGUMENT", message)
