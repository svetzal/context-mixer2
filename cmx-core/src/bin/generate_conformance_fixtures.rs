//! Regenerates the golden conformance fixtures under `cmx-core/conformance/` from
//! the in-memory `test-support` oracle. Run with `cargo run --bin
//! generate_conformance_fixtures --features test-support` after a behavior change
//! that the fixtures must reflect.

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance");
    cmx_core::conformance::generate_conformance_fixtures(&out)
}
