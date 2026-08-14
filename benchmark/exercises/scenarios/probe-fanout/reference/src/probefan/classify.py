"""Pure classification of an observed status against the caller's expectations."""

from collections.abc import Iterable

from probefan.models import HEALTHY, UNHEALTHY

DEFAULT_EXPECTED_STATUSES = frozenset({200})


def classify(status: int, expected_statuses: Iterable[int]) -> str:
    """Return the outcome for a status the service actually returned."""
    return HEALTHY if status in frozenset(expected_statuses) else UNHEALTHY
