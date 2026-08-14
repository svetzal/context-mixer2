"""Values exchanged across the probing boundary."""

from dataclasses import dataclass

HEALTHY = "healthy"
UNHEALTHY = "unhealthy"
TIMEOUT = "timeout"
UNREACHABLE = "unreachable"


@dataclass(frozen=True)
class Probe:
    """One monitored service endpoint."""

    name: str
    url: str


@dataclass(frozen=True)
class ProbeResult:
    """What one probe observed."""

    name: str
    url: str
    outcome: str
    status: int | None
