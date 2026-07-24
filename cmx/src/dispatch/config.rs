//! `cmx config` command dispatch, a submodule of `cmx/src/dispatch/mod.rs`.

use anyhow::Result;

use crate::cli::{ConfigAction, ExternalAction, HomeAction, PlatformsAction};
use crate::context::AppContext;

use super::print_json;

/// Dispatch `cmx home` subcommands (`init`, `path`).
pub fn handle_home(action: &HomeAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        HomeAction::Init => {
            let home = crate::adopt::home_init(ctx)?;
            println!("Canonical home ready at {}", home.display());
            println!("Registered as source '{}'.", crate::adopt::HOME_SOURCE);
            Ok(())
        }
        HomeAction::Path { output } => {
            let home = crate::adopt::home_path(ctx)?;
            if output.json {
                print_json(&crate::display::json::home_path_json(&home))?;
            } else {
                println!("{}", home.display());
            }
            Ok(())
        }
    }
}

/// Dispatch `cmx config` subcommands (show, gateway, model, external, platforms).
pub fn handle_config(action: ConfigAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ConfigAction::Show { output } => {
            let result = crate::cmx_config::show(ctx)?;
            if output.json {
                print_json(&crate::display::json::config_show_json(&result))?;
            } else {
                print!("{result}");
            }
            Ok(())
        }
        ConfigAction::Gateway { value } => {
            let result = crate::cmx_config::set_gateway(&value, ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::Model { value } => {
            let result = crate::cmx_config::set_model(&value, ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::External { action } => {
            let result = match action {
                ExternalAction::List => crate::cmx_config::external_list(ctx)?,
                ExternalAction::Add { entry } => crate::cmx_config::external_add(&entry, ctx)?,
                ExternalAction::Remove { entry } => {
                    crate::cmx_config::external_remove(&entry, ctx)?
                }
            };
            print!("{result}");
            Ok(())
        }
        ConfigAction::Platforms { action } => {
            let result = match action {
                PlatformsAction::List => crate::cmx_config::platforms_list(ctx)?,
                PlatformsAction::Add { platform } => {
                    crate::cmx_config::platforms_add(platform, ctx)?
                }
                PlatformsAction::Remove { platform } => {
                    crate::cmx_config::platforms_remove(platform, ctx)?
                }
            };
            print!("{result}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ExternalAction;
    use crate::dispatch::test_support::{fake_trio, make_test_ctx, no_json};

    #[test]
    fn handle_config_show_default_config_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_config(ConfigAction::Show { output: no_json() }, &ctx).is_ok());
    }

    #[test]
    fn handle_config_gateway_openai_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_config(
                ConfigAction::Gateway {
                    value: "openai".to_string()
                },
                &ctx
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_config_model_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_config(
                ConfigAction::Model {
                    value: "gpt-4".to_string()
                },
                &ctx
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_config_external_list_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_config(
                ConfigAction::External {
                    action: ExternalAction::List
                },
                &ctx
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_config_external_add_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_config(
                ConfigAction::External {
                    action: ExternalAction::Add {
                        entry: "my-skill".to_string()
                    }
                },
                &ctx
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_config_external_remove_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_config(
                ConfigAction::External {
                    action: ExternalAction::Remove {
                        entry: "my-skill".to_string()
                    }
                },
                &ctx
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_home_path_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_home(&HomeAction::Path { output: no_json() }, &ctx).is_ok());
    }

    #[test]
    fn handle_home_init_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_home(&HomeAction::Init, &ctx).is_ok());
    }
}
