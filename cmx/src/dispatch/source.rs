use anyhow::Result;

use crate::cli::SourceAction;
use crate::context::AppContext;
use crate::paths::ConfigPaths;

use super::print_json;

pub fn handle_source(
    action: SourceAction,
    paths: &ConfigPaths,
    ctx: &AppContext<'_>,
) -> Result<()> {
    match action {
        SourceAction::Add { name, path_or_url } => {
            if crate::source::looks_like_url(&path_or_url) {
                let clone_dir = paths.git_clones_dir().join(&name);
                println!("Cloning {path_or_url} to {}...", clone_dir.display());
            }
            let result = crate::source::add(&name, &path_or_url, ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::List { output } => {
            let result = crate::source::list(ctx)?;
            if output.json {
                print_json(&crate::display::json::source_list_json(&result))?;
            } else {
                print!("{result}");
            }
            Ok(())
        }
        SourceAction::Browse { name, output } => {
            let result = crate::source::browse(&name, ctx)?;
            if output.json {
                print_json(&crate::display::json::source_browse_json(&result))?;
            } else {
                print!("{result}");
            }
            Ok(())
        }
        SourceAction::Update { name } => {
            let output = crate::source_update::update(name.as_deref(), ctx)?;
            print!("{output}");
            Ok(())
        }
        SourceAction::Remove { name } => {
            let result = crate::source::remove(&name, ctx)?;
            print!("{result}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::test_support::{fake_trio, make_test_ctx, no_json};

    #[test]
    fn handle_source_list_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_source(SourceAction::List { output: no_json() }, &paths, &ctx).is_ok());
    }
}
