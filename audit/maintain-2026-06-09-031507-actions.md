Done. Here's a summary of what was updated and verified:

**Updates applied** (8 packages, all patch-level):
- `http` 1.4.1 → 1.4.2
- `wasm-bindgen` 0.2.122 → 0.2.123 (+ its macro, macro-support, shared, and futures crates)
- `js-sys` 0.3.99 → 0.3.100
- `web-sys` 0.3.99 → 0.3.100

**Quality gates** — all green:
- ✅ `cargo fmt --check`
- ✅ `cargo clippy --all-targets --all-features -- -D warnings`
- ✅ `cargo test --all-features` (all tests pass, including doctests)
- ✅ `cargo deny check` (advisories ok, bans ok, licenses ok, sources ok)

The `cargo deny` duplicate-crate warnings are pre-existing noise from `mojentic`'s transitive dependency tree (multiple major versions of `sha`/`digest`/`getrandom` in the same tree), not introduced by this update. No security advisories were triggered.