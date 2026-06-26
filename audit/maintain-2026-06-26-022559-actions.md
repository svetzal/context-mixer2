Done. Here's a summary of what was updated:

**Dependency updates applied:**
| Crate | Before | After |
|-------|--------|-------|
| `anyhow` | 1.0.102 | 1.0.103 |
| `bstr` | 1.12.2 | 1.12.3 |

**Held back (correctly, by MSRV 1.85):**
- `icu_*` family — require Rust 1.86
- `wasip2` — requires Rust 1.87
- `generic-array` — transitively constrained

**Quality gates:** all green — format ✓, clippy ✓, tests (863 total) ✓, coverage 89.50% ✓, `cargo deny` ✓

Committed to `main` and pushed. There are also unrelated pre-existing working-tree changes (`cmx/src/scan/frontmatter.rs` and `cmx/src/diff/`) left untouched — they appear to be from a prior maintenance session.