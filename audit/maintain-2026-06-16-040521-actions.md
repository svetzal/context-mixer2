Dependency update complete. Here's the summary:

**Changes applied:**
- `h2` 0.4.14 → 0.4.15 (patch, via `cargo update`) — a transitive dep of `reqwest` → `mojentic`

**Held back (by MSRV 1.85):**
- `icu_*`, `idna_adapter` — available patches require Rust 1.86
- `wasip2`, `wasip3`, `wit-bindgen` — available updates require Rust 1.87

**Quality gates:** all green
- ✅ `cargo fmt --check`
- ✅ `cargo clippy --all-targets --all-features -- -D warnings`
- ✅ `cargo test --all-features` (all tests pass)
- ✅ `cargo deny check` (advisories/bans/licenses/sources all ok)
- ✅ `cargo tarpaulin` — 88.88% coverage, well above the 80% threshold

The duplicate warnings from `cargo deny` (`block-buffer`, `core-foundation`, `digest`, `getrandom`, etc.) are pre-existing transitive conflicts from `mojentic`'s dependency tree — not introduced by this update and not actionable without upstream changes.