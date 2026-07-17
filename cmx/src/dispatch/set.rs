use anyhow::Result;
use std::process::ExitCode;

use crate::cli::{OutputArgs, SetAction};
use crate::context::AppContext;
use crate::flags::{Force, Purge, RunMode};
use crate::types::InstallScope;

use super::{print_json, usage_error};

pub(crate) const DRY_RUN_DEPRECATED_WARNING: &str =
    "--dry-run is deprecated; the plan is now shown by default — pass --apply to execute";

pub fn scope_from(local: bool) -> InstallScope {
    if local {
        InstallScope::Local
    } else {
        InstallScope::Global
    }
}

pub fn handle_set(action: SetAction, ctx: &AppContext<'_>) -> Result<ExitCode> {
    match action {
        SetAction::Create {
            name,
            desc,
            from_plugin,
            deprecated_from,
            local,
        } => {
            if deprecated_from.is_some() {
                eprintln!("--from is deprecated; use --from-plugin");
            }
            handle_set_create(
                &name,
                desc.as_deref(),
                from_plugin.as_deref().or(deprecated_from.as_deref()),
                local,
                ctx,
            )
        }
        SetAction::List { local, output } => handle_set_list(local, output, ctx),
        SetAction::Show {
            name,
            local,
            output,
        } => handle_set_show(&name, local, output, ctx),
        SetAction::Add {
            name,
            artifacts,
            local,
        } => handle_set_add(&name, &artifacts, local, ctx),
        SetAction::Remove {
            name,
            artifacts,
            local,
        } => handle_set_remove(&name, &artifacts, local, ctx),
        SetAction::Activate {
            name,
            apply,
            dry_run,
            local,
        } => {
            let mode = if dry_run {
                eprintln!("{DRY_RUN_DEPRECATED_WARNING}");
                RunMode::Plan
            } else {
                RunMode::from_flag(apply)
            };
            handle_set_activate(&name, mode, scope_from(local), ctx)
        }
        SetAction::Deactivate {
            name,
            apply,
            dry_run,
            force,
            local,
        } => {
            let mode = if dry_run {
                eprintln!("{DRY_RUN_DEPRECATED_WARNING}");
                RunMode::Plan
            } else {
                RunMode::from_flag(apply)
            };
            handle_set_deactivate(&name, mode, Force::from_flag(force), scope_from(local), ctx)
        }
        SetAction::Delete {
            name,
            local,
            purge,
            apply,
            force,
        } => {
            let result = crate::sets::delete(
                &name,
                Purge::from_flag(purge),
                Force::from_flag(force),
                RunMode::from_flag(apply),
                scope_from(local),
                ctx,
            )?;
            let deleted = result.deleted;
            let preview = result.purge && !result.apply;
            print!("{result}");
            Ok(if deleted || preview {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        SetAction::Rename { old, new, local } => handle_set_rename(&old, &new, local, ctx),
    }
}

fn handle_set_create(
    name: &str,
    desc: Option<&str>,
    from: Option<&str>,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = crate::sets::create(name, desc, from, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_list(local: bool, output: OutputArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let scope = scope_from(local);
    let result = crate::sets::list(scope, ctx)?;
    if output.json {
        print_json(&crate::display::json::set_list_json(&result, scope))?;
    } else {
        print!("{result}");
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_set_show(
    name: &str,
    local: bool,
    output: OutputArgs,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let scope = scope_from(local);
    let result = crate::sets::show(name, scope, ctx)?;
    if output.json {
        print_json(&crate::display::json::set_show_json(&result, scope))?;
    } else {
        print!("{result}");
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_set_add(
    name: &str,
    artifacts: &[String],
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    if artifacts.is_empty() {
        return Err(usage_error(
            "Provide artifact name(s) to add to the set",
            &format!("cmx set add {name} <artifact>"),
        ));
    }
    let result = crate::sets::add(name, artifacts, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_remove(
    name: &str,
    artifacts: &[String],
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    if artifacts.is_empty() {
        return Err(usage_error(
            "Provide artifact name(s) to remove from the set",
            &format!("cmx set remove {name} <artifact>"),
        ));
    }
    let result = crate::sets::remove(name, artifacts, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_activate(
    name: &str,
    mode: RunMode,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = crate::sets::activate(name, mode, scope, ctx)?;
    let any_failed = result.any_failed;
    print!("{result}");
    Ok(if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn handle_set_deactivate(
    name: &str,
    mode: RunMode,
    force: Force,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = crate::sets::deactivate(name, force, mode, scope, ctx)?;
    let any_blocked = result.any_blocked;
    print!("{result}");
    Ok(if any_blocked {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn handle_set_rename(old: &str, new: &str, local: bool, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let result = crate::sets::rename(old, new, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::dispatch::test_support::{fake_trio, make_test_ctx, no_json};
    use crate::types::{ArtifactKind, SetMember};
    use cmx_core::test_support::setup_source_with_agent;

    #[test]
    fn handle_set_add_empty_artifacts_errors_with_try_line() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_set_add("daily", &[], false, &ctx);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("try: cmx set add daily <artifact>"),
            "missing set add hint"
        );
    }

    #[test]
    fn handle_set_remove_empty_artifacts_errors_with_try_line() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_set_remove("daily", &[], false, &ctx);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("try: cmx set remove daily <artifact>"),
            "missing set remove hint"
        );
    }

    #[test]
    fn handle_set_list_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_set(
                SetAction::List {
                    local: false,
                    output: no_json(),
                },
                &ctx
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_set_create_then_show_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(
            handle_set(
                SetAction::Create {
                    name: "rust-work".to_string(),
                    desc: None,
                    from_plugin: None,
                    deprecated_from: None,
                    local: false,
                },
                &ctx,
            )
            .is_ok()
        );
        assert!(
            handle_set(
                SetAction::Show {
                    name: "rust-work".to_string(),
                    local: false,
                    output: no_json(),
                },
                &ctx,
            )
            .is_ok()
        );
    }

    #[test]
    fn handle_set_delete_with_purge_deactivates_then_deletes() {
        let (fs, git, clock, paths) = fake_trio();
        setup_source_with_agent(&fs, &paths, "guidelines", "/src", "rust-craftsperson");
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        handle_set(
            SetAction::Create {
                name: "rust-work".to_string(),
                desc: None,
                from_plugin: None,
                deprecated_from: None,
                local: false,
            },
            &ctx,
        )
        .unwrap();
        config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| -> Result<()> {
            sets.sets.get_mut("rust-work").unwrap().members.push(SetMember {
                kind: ArtifactKind::Agent,
                name: "rust-craftsperson".to_string(),
                source: Some("guidelines".to_string()),
            });
            Ok(())
        })
        .unwrap();
        handle_set(
            SetAction::Activate {
                name: "rust-work".to_string(),
                apply: true,
                dry_run: false,
                local: false,
            },
            &ctx,
        )
        .unwrap();

        let result = handle_set(
            SetAction::Delete {
                name: "rust-work".to_string(),
                local: false,
                purge: true,
                apply: true,
                force: false,
            },
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
        let sets = config::load_sets(InstallScope::Global, &fs, &paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn handle_set_delete_with_purge_plan_returns_success_without_deleting() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        handle_set(
            SetAction::Create {
                name: "rust-work".to_string(),
                desc: None,
                from_plugin: None,
                deprecated_from: None,
                local: false,
            },
            &ctx,
        )
        .unwrap();

        let result = handle_set(
            SetAction::Delete {
                name: "rust-work".to_string(),
                local: false,
                purge: true,
                apply: false,
                force: false,
            },
            &ctx,
        );

        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
        let sets = config::load_sets(InstallScope::Global, &fs, &paths).unwrap();
        assert!(sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn handle_set_activate_unresolvable_source_returns_failure_exit_code() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        handle_set(
            SetAction::Create {
                name: "rust-work".to_string(),
                desc: None,
                from_plugin: None,
                deprecated_from: None,
                local: false,
            },
            &ctx,
        )
        .unwrap();
        config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| -> Result<()> {
            sets.sets.get_mut("rust-work").unwrap().members.push(SetMember {
                kind: ArtifactKind::Skill,
                name: "ghost".to_string(),
                source: Some("gone".to_string()),
            });
            Ok(())
        })
        .unwrap();

        let result = handle_set(
            SetAction::Activate {
                name: "rust-work".to_string(),
                apply: true,
                dry_run: false,
                local: false,
            },
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::FAILURE);
    }

    #[test]
    fn handle_set_deactivate_drift_blocked_returns_failure_exit_code() {
        let (fs, git, clock, paths) = fake_trio();
        setup_source_with_agent(&fs, &paths, "guidelines", "/src", "rust-craftsperson");
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        handle_set(
            SetAction::Create {
                name: "rust-work".to_string(),
                desc: None,
                from_plugin: None,
                deprecated_from: None,
                local: false,
            },
            &ctx,
        )
        .unwrap();
        config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| -> Result<()> {
            sets.sets.get_mut("rust-work").unwrap().members.push(SetMember {
                kind: ArtifactKind::Agent,
                name: "rust-craftsperson".to_string(),
                source: Some("guidelines".to_string()),
            });
            Ok(())
        })
        .unwrap();
        handle_set(
            SetAction::Activate {
                name: "rust-work".to_string(),
                apply: true,
                dry_run: false,
                local: false,
            },
            &ctx,
        )
        .unwrap();
        let installed_path = paths
            .installed_artifact_path(ArtifactKind::Agent, "rust-craftsperson", InstallScope::Global)
            .unwrap();
        fs.add_file(installed_path, "edited by hand");

        let result = handle_set(
            SetAction::Deactivate {
                name: "rust-work".to_string(),
                apply: true,
                dry_run: false,
                force: false,
                local: false,
            },
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::FAILURE);
    }

    #[test]
    fn handle_set_activate_dry_run_returns_success() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        handle_set(
            SetAction::Create {
                name: "rust-work".to_string(),
                desc: None,
                from_plugin: None,
                deprecated_from: None,
                local: false,
            },
            &ctx,
        )
        .unwrap();
        let result = handle_set(
            SetAction::Activate {
                name: "rust-work".to_string(),
                apply: false,
                dry_run: true,
                local: false,
            },
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}
