All quality gates pass. Here's the dependency maintenance summary:

## Dependency Maintenance Report

**Result: No updates needed — all dependencies already current**

`cargo update --dry-run` confirmed all lock-file entries are already at their latest MSRV-compatible versions. The Cargo.lock had 0 packages to update.

### Gates

| Gate | Result |
|------|--------|
| `cargo fmt --check` | ✅ Pass |
| `cargo clippy --all-targets --all-features` | ✅ Pass |
| `cargo test --all-features` | ✅ Pass |
| `cargo tarpaulin --all-features --fail-under 64` | ✅ 79.61% coverage |
| `cargo deny check` | ✅ advisories/bans/licenses/sources all ok |
| `cargo audit` | ✅ No vulnerabilities |

### Packages held back by MSRV

There are 9 packages with newer versions available that require **Rust 1.86+**, which is above the current MSRV of 1.85:

| Crate | Current | Available | Requires |
|-------|---------|-----------|----------|
| `icu_collections` | 2.1.1 | 2.2.0 | Rust 1.86 |
| `icu_locale_core` | 2.1.1 | 2.2.0 | Rust 1.86 |
| `icu_normalizer` | 2.1.1 | 2.2.0 | Rust 1.86 |
| `icu_properties` | 2.1.2 | 2.2.0 | Rust 1.86 |
| `icu_provider` | 2.1.1 | 2.2.0 | Rust 1.86 |
| `idna_adapter` | 1.2.1 | 1.2.2 | Rust 1.86 |
| `wasip2` | 1.0.1 | 1.0.3 | Rust 1.87 |
| `wasip3` | 0.4.0 | — | Rust 1.87 |
| `wit-bindgen` | 0.51.0 | — | Rust 1.87 |

These are all transitive dependencies pulled in through `mojentic`/`reqwest`. To pick them up, the workspace `rust-version` in `Cargo.toml` would need to be bumped to at least 1.86 — let me know if you'd like to do that.