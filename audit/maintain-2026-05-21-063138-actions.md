Dependency update complete. Here's the summary:

**What changed**
- `crypto-common` `0.2.1 → 0.2.2` — patch update to a transitive dependency of `sha2`

**Why so little?**
The project is pinned to MSRV `1.85`. Nine other packages have newer versions available (`icu_*`, `idna_adapter`, `wasip2`, `wasip3`, `wit-bindgen`), but all require Rust 1.86 or 1.87 — so `cargo update` correctly left them alone. All direct dependencies (`clap`, `serde`, `tokio`, `chrono`, `mojentic`, etc.) are already at their latest versions within their declared semver ranges.

**Quality gates** — all green:
| Gate | Result |
|------|--------|
| `cargo fmt --check` | ✅ |
| `cargo clippy --all-targets --all-features` | ✅ |
| `cargo test --all-features` | ✅ (all tests pass) |
| `cargo tarpaulin --fail-under 64` | ✅ 80.26% coverage |
| `cargo deny check` | ✅ (duplicate warnings are pre-existing transitive conflicts from `mojentic`) |