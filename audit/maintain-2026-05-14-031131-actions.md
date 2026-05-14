Dependency maintenance complete.

**Summary:**
- **Direct dependencies** — all already at latest compatible versions (`cargo outdated` reported clean)
- **Transitive updates applied** — `aws-lc-rs` 1.16.3 → 1.17.0 and `aws-lc-sys` 0.40.0 → 0.41.0 (patch updates, pulled in via `mojentic` → `rustls`)
- **9 transitive deps held back** — `icu_*`, `idna_adapter`, `wasip2/3`, `wit-bindgen` all require Rust 1.86–1.87; MSRV is pinned at 1.85 so they stay put
- **All 5 quality gates pass** — format, lint (0 warnings), tests, coverage (82.99%), deny