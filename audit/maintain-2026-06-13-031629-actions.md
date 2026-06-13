Dependency update complete. Here's a summary:

**`cargo outdated` reported no semver-range updates** — all declared versions in `Cargo.toml` already resolve to the latest compatible releases. `cargo update` picked up **14 patch-level lockfile refreshes** across transitive deps:

| Package | Old → New |
|---|---|
| `block-buffer` | 0.12.0 → 0.12.1 |
| `cc` | 1.2.63 → 1.2.64 |
| `js-sys` / `web-sys` | 0.3.100 → 0.3.102 |
| `wasm-bindgen` (+ macro/shared/futures) | 0.2.123 → 0.2.125 |
| `memchr` | 2.8.1 → 2.8.2 |
| `openssl` | 0.10.80 → 0.10.81 |
| `openssl-sys` | 0.9.116 → 0.9.117 |
| `smallvec` | 1.15.1 → 1.15.2 |
| `zeroize` | 1.8.2 → 1.9.0 |

All five quality gates passed (format ✅, clippy ✅, tests ✅, deny ✅, coverage 88.83% ✅). Changes pushed to `origin/main`.