Dependency maintenance complete. Here's a summary:

**Update applied:**
- `hybrid-array` v0.4.11 → v0.4.12 (patch update, part of the `sha2` / crypto ecosystem via `sha-2`)

**Held back (MSRV constraint — require Rust 1.86 or 1.87):**
- `icu_*` family (v2.1.x → v2.2.0, requires Rust 1.86)
- `idna_adapter` (v1.2.1 → v1.2.2, requires Rust 1.86)
- `wasip2`, `wasip3`, `wit-bindgen` (requires Rust 1.87)

**Quality gates:** All five passed — format ✅, clippy ✅, tests ✅ (all pass), coverage ✅ (82.4% > 64% threshold), deny ✅.