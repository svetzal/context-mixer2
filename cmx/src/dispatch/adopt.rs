use anyhow::Result;
use std::path::Path;

use crate::context::AppContext;
use crate::types::ArtifactKind;

use super::usage_error;

pub fn handle_unadopt(
    names: &[String],
    kind: ArtifactKind,
    external: bool,
    ctx: &AppContext<'_>,
) -> Result<()> {
    if names.is_empty() {
        return Err(usage_error(
            "Provide artifact name(s) to unadopt",
            &format!("cmx {kind} unadopt <name>"),
        ));
    }
    let outcome = crate::adopt::unadopt_many(names, kind, ctx)?;
    print!("{outcome}");
    if external {
        for name in names {
            let r = crate::cmx_config::external_add(name, ctx)?;
            print!("{r}");
        }
    }
    Ok(())
}

pub fn handle_adopt(
    names: &[String],
    kind: ArtifactKind,
    all: bool,
    from: Option<&Path>,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let outcome = if all {
        crate::adopt::adopt_all(Some(kind), from, local, ctx)?
    } else if names.is_empty() {
        return Err(usage_error(
            "Provide artifact name(s) to adopt, or use --all",
            &format!("cmx {kind} adopt <name>"),
        ));
    } else {
        crate::adopt::adopt_named(kind, names, local, ctx)?
    };
    print!("{outcome}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::test_support::{fake_trio, make_test_ctx};

    #[test]
    fn handle_unadopt_empty_names_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_unadopt(&[], ArtifactKind::Agent, false, &ctx);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("try: cmx agent unadopt <name>"),
            "missing unadopt hint"
        );
    }

    #[test]
    fn handle_adopt_empty_names_no_all_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_adopt(&[], ArtifactKind::Agent, false, None, false, &ctx);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("try: cmx agent adopt <name>"),
            "missing adopt hint"
        );
    }
}
