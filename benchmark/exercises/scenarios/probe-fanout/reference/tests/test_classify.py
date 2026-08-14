import pytest

from probefan.classify import DEFAULT_EXPECTED_STATUSES, classify


@pytest.mark.parametrize(
    ("status", "expected_statuses", "outcome"),
    [
        (200, DEFAULT_EXPECTED_STATUSES, "healthy"),
        (201, DEFAULT_EXPECTED_STATUSES, "unhealthy"),
        (418, DEFAULT_EXPECTED_STATUSES, "unhealthy"),
        (500, DEFAULT_EXPECTED_STATUSES, "unhealthy"),
        (418, {418}, "healthy"),
        (200, {418}, "unhealthy"),
        (204, {200, 204}, "healthy"),
    ],
)
def test_status_classification(status, expected_statuses, outcome):
    assert classify(status, expected_statuses) == outcome
