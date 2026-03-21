Everything is fine. Both diagnostics are IDE noise, not real issues:

- **`diff.rs` "unused import: `crate::fs_util`"** — A stale rust-analyzer diagnostic. Clippy passes cleanly; the import is legitimately used at line 158 (`fs_util::collect_files(dir)?`). This appears when rust-analyzer's analysis cache is out of sync with the compiler's view.

- **"This file is not included in any crates"** for `scan_integration.rs` and `checksum_integration.rs`** — rust-analyzer sometimes doesn't auto-discover files placed directly in `tests/` until it re-indexes. Cargo finds and runs them correctly (10 scan integration tests shown above). These are not `mod` declarations — Cargo picks them up automatically as integration test targets.

No code changes needed. All 100 tests pass, zero clippy warnings, formatting clean, deny check green.