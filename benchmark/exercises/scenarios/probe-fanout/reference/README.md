# probefan

Concurrent health probing for the service dashboard.

The dashboard refreshes on a fixed interval and shows one row per monitored
service. A slow or unreachable service must not delay the rest of the refresh,
so probing fans out and each probe is bounded.

## Development

```bash
uv sync
uv run pytest
```
