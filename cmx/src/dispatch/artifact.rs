//! `cmx agent` / `cmx skill` artifact command dispatch, a submodule of
//! `cmx/src/dispatch/mod.rs`.

use anyhow::Result;
use std::process::ExitCode;

use crate::cli::ArtifactAction;
use crate::context::AppContext;
use crate::dispatch::scope_from;
use crate::dispatch::set::DRY_RUN_DEPRECATED_WARNING;
use crate::flags::{Force, RunMode, Selection};
use crate::platform::{Platform, platforms_label};
use crate::types::{ArtifactKind, InstallScope};

use super::{print_json, usage_error};

/// Dispatch `cmx agent install` / `cmx skill install`: refresh stale sources,
/// then install either every eligible artifact (`--all`) or the named ones.
pub fn handle_install(
    names: &[String],
    all: Selection,
    scope: InstallScope,
    force: Force,
    kind: ArtifactKind,
    selector: Option<Platform>,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    crate::source_update::ensure_fresh(ctx)?;
    let targets = crate::install::resolve_targets(selector, kind, scope, ctx)?;
    if all.is_all() {
        let result = crate::install::install_all(kind, scope, force, &targets, ctx)?;
        print!("{result}");
        Ok(ExitCode::SUCCESS)
    } else if names.is_empty() {
        Err(usage_error(
            "Provide artifact name(s) or use --all",
            &format!("cmx {kind} install <name>"),
        ))
    } else {
        let result = crate::install::install_many(names, kind, scope, force, &targets, ctx)?;
        let any_failed = !result.failed.is_empty();
        print!("{result}");
        Ok(if any_failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        })
    }
}

/// Dispatch `cmx agent update` / `cmx skill update`: refresh stale sources,
/// then update either every eligible artifact (`--all`) or the named one,
/// warning about sibling platforms left out of sync when relevant.
pub fn handle_update(
    name: Option<String>,
    all: Selection,
    force: Force,
    kind: ArtifactKind,
    selector: Option<Platform>,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    crate::source_update::ensure_fresh(ctx)?;
    if all.is_all() {
        let result = crate::install::update_all(kind, force, ctx)?;
        print!("{result}");
        Ok(ExitCode::SUCCESS)
    } else if let Some(name) = name {
        let result = crate::install::update(&name, kind, force, ctx)?;
        print!("{result}");
        if selector.is_none() && !result.sibling_drifted_platforms.is_empty() {
            print_update_note(
                &name,
                kind,
                result.updated.platform,
                &result.sibling_drifted_platforms,
            );
        }
        Ok(ExitCode::SUCCESS)
    } else {
        Err(usage_error(
            "Provide an artifact name or use --all",
            &format!("cmx {kind} update <name>"),
        ))
    }
}

fn print_update_note(
    name: &str,
    kind: ArtifactKind,
    updated_platform: Platform,
    sibling_platforms: &[Platform],
) {
    let sibling_list = platforms_label(sibling_platforms);
    match kind {
        ArtifactKind::Skill => eprintln!(
            "note: '{name}' is also installed on {sibling_list} and remains out of sync there; \
             'update' only targets {updated_platform}. Reconcile the other copies with \
             'cmx skill sync {name}'."
        ),
        ArtifactKind::Agent => eprintln!(
            "note: '{name}' is also installed on {sibling_list} and remains drifted there; \
             'update' only targets {updated_platform}. Re-run 'cmx agent update {name} \
             --platform <platform> --force' for each remaining platform."
        ),
    }
}

/// Dispatch `cmx agent uninstall` / `cmx skill uninstall` for one or more
/// named artifacts.
pub fn handle_uninstall(
    names: &[String],
    scope: InstallScope,
    kind: ArtifactKind,
    selector: Option<Platform>,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    if names.is_empty() {
        return Err(usage_error(
            "Provide artifact name(s) to uninstall",
            &format!("cmx {kind} uninstall <name>"),
        ));
    }
    let result = crate::uninstall::uninstall_many(names, kind, scope, selector, ctx)?;
    let none_removed = result.removed.is_empty();
    print!("{result}");
    Ok(if none_removed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Top-level dispatch for `cmx agent <action>` / `cmx skill <action>`, routing
/// to the matching `handle_*` function for the given kind.
pub fn handle_artifact(
    action: ArtifactAction,
    kind: ArtifactKind,
    selector: Option<Platform>,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    match action {
        ArtifactAction::Install {
            names,
            all,
            local,
            force,
        } => handle_install(
            &names,
            Selection::from_flag(all),
            scope_from(local),
            Force::from_flag(force),
            kind,
            selector,
            ctx,
        ),
        ArtifactAction::List { all, output } => {
            let report = crate::list::list_kind(kind, all, ctx)?;
            if output.json {
                print_json(&crate::display::json::list_kind_json(&report))?;
            } else {
                print!("{report}");
            }
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Info { name, output } => {
            super::info::handle_info(&name, Some(kind), output.json, ctx)
                .map(|()| ExitCode::SUCCESS)
        }
        ArtifactAction::Diff { name, full } => {
            crate::source_update::ensure_fresh(ctx)?;
            let output = super::diff::handle_diff(&name, kind, full, ctx)?;
            print!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Update { name, all, force } => handle_update(
            name,
            Selection::from_flag(all),
            Force::from_flag(force),
            kind,
            selector,
            ctx,
        ),
        ArtifactAction::Sync {
            name,
            from,
            apply,
            dry_run,
            local,
        } => {
            let scope = scope_from(local);
            let mode = if dry_run {
                eprintln!("{DRY_RUN_DEPRECATED_WARNING}");
                RunMode::Plan
            } else {
                RunMode::from_flag(apply)
            };
            let result = crate::sync::sync(&name, kind, scope, from, mode, ctx)?;
            print!("{result}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Promote { name, from, apply } => {
            let mode = RunMode::from_flag(apply);
            let result = crate::promote::promote(&name, kind, from.or(selector), mode, ctx)?;
            print!("{result}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Uninstall { names, local } => {
            handle_uninstall(&names, scope_from(local), kind, selector, ctx)
        }
        ArtifactAction::Unadopt { names, external } => {
            super::adopt::handle_unadopt(&names, kind, external, ctx).map(|()| ExitCode::SUCCESS)
        }
        ArtifactAction::Adopt {
            names,
            all,
            from_dir,
            deprecated_from,
            local,
        } => {
            if deprecated_from.is_some() {
                eprintln!("--from is deprecated; use --from-dir");
            }
            super::adopt::handle_adopt(
                &names,
                kind,
                Selection::from_flag(all),
                from_dir.as_deref().or(deprecated_from.as_deref()),
                crate::flags::SurveyScope::from_flag(local),
                ctx,
            )
            .map(|()| ExitCode::SUCCESS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::test_support::{fake_trio, make_test_ctx};
    use crate::gateway::Filesystem;
    use crate::lockfile;
    use crate::platform::Platform;
    use crate::types::LockEntry;

    #[test]
    fn handle_artifact_install_empty_names_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_artifact(
            ArtifactAction::Install {
                names: vec![],
                all: false,
                local: false,
                force: false,
            },
            ArtifactKind::Agent,
            None,
            &ctx,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("all"));
        assert!(err.contains("try: cmx agent install <name>"), "{err}");
    }

    #[test]
    fn handle_artifact_install_not_found_returns_failure_exit_code() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        // "nonexistent" is not in any source → install_many puts it in `failed`
        // → previously process::exit(1), now returns Ok(ExitCode::FAILURE)
        let result = handle_artifact(
            ArtifactAction::Install {
                names: vec!["nonexistent".to_string()],
                all: false,
                local: false,
                force: false,
            },
            ArtifactKind::Agent,
            None,
            &ctx,
        );
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::FAILURE);
    }

    #[test]
    fn handle_artifact_update_no_name_no_all_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_artifact(
            ArtifactAction::Update {
                name: None,
                all: false,
                force: false,
            },
            ArtifactKind::Agent,
            None,
            &ctx,
        );
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("try: cmx agent update <name>"),
            "missing update hint"
        );
    }

    #[test]
    fn handle_artifact_uninstall_empty_names_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_artifact(
            ArtifactAction::Uninstall {
                names: vec![],
                local: false,
            },
            ArtifactKind::Agent,
            None,
            &ctx,
        );
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("try: cmx agent uninstall <name>"),
            "missing uninstall hint"
        );
    }

    #[test]
    fn handle_promote_prefers_from_over_global_platform_selector() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);

        let claude_paths = paths.with_platform(Platform::Claude);
        let codex_paths = paths.with_platform(Platform::Codex);
        let claude_skill = claude_paths
            .installed_artifact_path(ArtifactKind::Skill, "pf", InstallScope::Global)
            .unwrap();
        let codex_skill = codex_paths
            .installed_artifact_path(ArtifactKind::Skill, "pf", InstallScope::Global)
            .unwrap();
        fs.add_file(
            claude_skill.join("SKILL.md"),
            "---\ndescription: claude wins\n---\n# claude\n",
        );
        fs.add_file(codex_skill.join("SKILL.md"), "---\ndescription: codex loses\n---\n# codex\n");

        for (platform, skill_path) in [
            (Platform::Claude, &claude_skill),
            (Platform::Codex, &codex_skill),
        ] {
            let checksum =
                crate::checksum::checksum_artifact(skill_path, ArtifactKind::Skill, &fs).unwrap();
            let pv = paths.with_platform(platform);
            lockfile::mutate(InstallScope::Global, &fs, &pv, |lock| {
                lock.packages.insert(
                    "pf".to_string(),
                    LockEntry {
                        artifact_type: ArtifactKind::Skill,
                        version: Some("1.0.0".to_string()),
                        installed_at: "2026-07-05T00:00:00Z".to_string(),
                        source: crate::types::LockSource {
                            repo: crate::adopt::HOME_SOURCE.to_string(),
                            path: "skills/pf/SKILL.md".to_string(),
                        },
                        source_checksum: "sha256:stale".to_string(),
                        installed_checksum: if platform == Platform::Claude {
                            "sha256:stale".to_string()
                        } else {
                            checksum.clone()
                        },
                    },
                );
                Ok::<(), anyhow::Error>(())
            })
            .unwrap()
            .unwrap();
        }

        let result = handle_artifact(
            ArtifactAction::Promote {
                name: "pf".to_string(),
                from: Some(Platform::Claude),
                apply: true,
            },
            ArtifactKind::Skill,
            Some(Platform::Codex),
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);

        let home_skill = paths.config_dir.join("home").join("skills").join("pf").join("SKILL.md");
        let promoted = fs.read_to_string(&home_skill).unwrap();
        assert!(promoted.contains("claude wins"), "{promoted}");
        assert!(!promoted.contains("codex loses"), "{promoted}");
    }
}
