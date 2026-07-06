# Changelog — cmx-core

All notable changes to the `cmx-core` library will be documented in this file.
This covers **both** ports, which are released in lockstep at the same version:
the Rust crate (`cmx-core` on crates.io, tagged `cmx-core-v*`) and the
TypeScript port (`cmx-core` on npm, tagged `cmx-core-ts-v*`). See the repo
`AGENTS.md` § "cmx-core & cmx-core-ts" for the parity and release policy.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-07-06

### Added

- **Conformance suite as the cross-port parity contract.** `SPEC.md` plus shared
  golden fixtures under `conformance/` (checksum, frontmatter, version-guard,
  paths, target-resolve, install-e2e) now define the byte-for-byte behavior every
  port must satisfy. The Rust crate runs them via `cargo test` (`conformance.rs`);
  the TypeScript port runs the same fixtures via `bun test`. A port that lags the
  fixtures is a release blocker.

### Fixed

- **Canonical path ordering and checksum keying now use the `/`-joined
  relative-path string** (`rel_path_key`) rather than component-wise `Path`
  ordering. The two only differ at the `.`-vs-`/` boundary (`a` < `a.b` < `a/b`),
  where component-wise ordering diverged; keying both the sort and the hash stream
  on the same string keeps a skill's in-memory (bundled) checksum and its on-disk
  checksum in agreement, and normalizes `\`-separated paths a Windows port might
  produce (SPEC §5.1 / §11.3 / §11.4). Normal single/nested paths are unchanged,
  so existing checksums are unaffected.

### Changed

- Internal refactor: `skill_install` split into `plan`, `types`, and `display`
  submodules. No observable behavior change (covered by the install-e2e fixtures).

## [0.2.0] and earlier

Pre-changelog. `0.2.0` introduced the TypeScript port (`cmx-core-ts`) and the
`metadata.version` reconciliation delegated from `cmx init`; `0.1.0` was the
initial extraction of the embeddable installer core from the cmx CLI. See
`git log` and the `cmx-core-v0.1.0` / `cmx-core-v0.2.0` tags for detail.
