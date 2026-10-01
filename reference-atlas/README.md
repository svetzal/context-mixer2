# Reference intent atlas

Context-mixer2 owns this small corpus to develop the contract between cmf and
cmv. External atlases own production guidance. The CLIs read both kinds of atlas
without changing their records, profiles, sensors or validators.

The `rust-shipping` profile selects two executable Rust checks and one unchecked
documentation intent. The Python record exercises ecosystem exclusion and has a
source-based annotation check, calibrated against annotated and untyped cases. The corpus uses project-owned intent identifiers.

The Rust checks cover a deliberately small reference layout. `src/core.rs` must
compile without `std` or a gateway module. `src/shell.rs` must compile without
`std`, with `src/gateway.rs` and an entrypoint accepting its `Gateway` trait.
Each validator first compiles the same source normally. Invalid source or an
unavailable compiler produces unchecked evidence. It then uses `rustc` metadata
compilation with `no_std` to test the restriction. It never runs project code.
These checks do not prove purity or gateway use for arbitrary Rust projects.

`cases/compliant`, `cases/violation` and `cases/gateway-violation` contain actual
source differences. The adoption test introduces filesystem access into the
core or shell, verifies the identified failure, and restores compliant source.
It also checks preview purity, Codex installation, manifest fields, strict
unchecked behavior and verification against a pinned atlas after HEAD changes.

`regressions/compile-manifest` and `regressions/verifier` preserve the earlier
golden protocol cases. Their synthetic validators deliberately test config
handoff and verdict aggregation. They are protocol doubles, not source-adherence
evidence. Existing malformed-input and compatibility tests remain distinct.

Run from the repository root:

```sh
cargo build -p cmf -p cmv
cargo test -p cmf --test manifest_golden --test codex_platform
cargo test -p cmv --test project_adoption --test check_golden
```

The adoption loop needs Python 3, Rust and Git. It copies the corpus into a
temporary git repository and uses an isolated project and home. It does not
need a guidelines checkout, network access, credentials or an LLM.
