No changes — the lock file was already fully current. Here's a summary of what was found:

---

## Dependency Maintenance Summary

**Result: Lock file already up to date. All quality gates pass.**

### Current state

All direct dependencies are at their latest compatible versions within the current MSRV (`rust-version = "1.85"`):

| Crate | Cargo.toml spec | Locked version |
|-------|----------------|---------------|
| `clap` | `4` | 4.6.1 ✅ |
| `serde` | `1` | 1.0.228 ✅ |
| `serde_json` | `1` | 1.0.149 ✅ |
| `anyhow` | `1` | 1.0.102 ✅ |
| `sha2` | `0.11` | 0.11.0 ✅ |
| `chrono` | `0.4.44` | 0.4.44 ✅ |
| `tokio` | `1` | 1.52.3 ✅ |
| `dirs` | `6` | 6.0.0 ✅ |
| `tempfile` | `3` | 3.27.0 ✅ |
| `mojentic` | `1.2` | 1.4.0 ✅ |

### Available but held back (MSRV constraint)

9 transitive dependencies have newer versions, all blocked by MSRV:

- **icu_collections / icu_locale_core / icu_normalizer / icu_properties / icu_provider / idna_adapter** — 2.1.x → 2.2.x, requires Rust **1.86**
- **wasip2 / wit-bindgen (×2 versions) / wasip3** — requires Rust **1.87**

These all flow in via `mojentic` → `reqwest` → `url` → `idna` → `icu_*`. Since the toolchain is pinned to **1.94.0** in `rust-toolchain.toml`, raising `rust-version` from `1.85` to `1.87` in `Cargo.toml` would unlock all of them — but that's an MSRV invariant change requiring explicit authorization.

### Quality gate results

| Gate | Result |
|------|--------|
| `cargo fmt --check` | ✅ |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ |
| `cargo test --all-features` | ✅ |
| `cargo deny check` | ✅ (duplicate warnings are pre-existing, not errors) |
| `cargo tarpaulin --fail-under 64` | ✅ 79.6% coverage |