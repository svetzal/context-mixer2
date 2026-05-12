All done. Here's a summary of what was updated:

---

## Dependency Update Summary

**Direct dependency updated:**
- `mojentic` 1.2.0 → 1.4.0 (minor update, within the `"1.2"` semver range)

**Significant transitive changes brought in by mojentic 1.4:**

| Change | Before | After |
|--------|--------|-------|
| TLS backend | `native-tls` / `openssl` | `aws-lc-rs` (rustls) |
| HTTP client | `reqwest` 0.12 | `reqwest` 0.13 |
| Random | `rand` 0.8 | `rand` 0.9 |
| JSON schema | `schemars` 0.8 | `schemars` 1.x |
| Tokenizer | `tiktoken-rs` 0.5 | `tiktoken-rs` 0.11 |
| HTML scraping | `scraper`/`selectors` removed | — |

**`deny.toml` changes:**
- Added `CDLA-Permissive-2.0` to allowed licenses (for `webpki-root-certs`, Mozilla's CA bundle)
- Removed stale advisory exemptions for `dotenv` and `fxhash` (no longer in the tree)

**All quality gates passed:** fmt ✓ · clippy ✓ · tests ✓ · coverage 82.54% ✓ · deny ✓