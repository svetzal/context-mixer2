use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance");
    cmx_core::conformance::generate_conformance_fixtures(&out)
}
