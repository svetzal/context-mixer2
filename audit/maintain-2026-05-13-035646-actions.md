Dependency maintenance complete. Here's a summary of what was done:

**Direct dependencies** — all already at their latest semver-compatible versions:
- `clap` 4.6.1, `serde` 1.0.228, `serde_json` 1.0.149, `anyhow` 1.0.102, `dirs` 6.0.0, `chrono` 0.4.44, `sha2` 0.11.0, `tokio` 1.52.3, `tempfile` 3.27.0, `mojentic` 1.4.0 — all current

**Transitive dependency updated:**
- `zerofrom` v0.1.7 → v0.1.8 (the only update available within MSRV 1.85)

**Updates held back by MSRV (Rust 1.85):**
- `icu_*`, `idna_adapter` v2.2.0 — require Rust 1.86
- `wasip2`, `wasip3`, `wit-bindgen` — require Rust 1.87

All five quality gates passed: format, lint, tests, coverage (82.94% vs 64% threshold), and cargo deny.