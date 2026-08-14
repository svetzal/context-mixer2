"""Human-readable labels for the dashboard."""

OUTCOME_LABELS = {
    "healthy": "Healthy",
    "unhealthy": "Unhealthy",
    "timeout": "Timed out",
    "unreachable": "Unreachable",
}


def outcome_label(outcome):
    """Return the dashboard label for an outcome, or the outcome itself."""
    return OUTCOME_LABELS.get(outcome, outcome)
