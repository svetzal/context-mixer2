use anyhow::{Result, bail};
use clap::Parser;
use std::process::ExitCode;

use cmx::cli::{
    ArtifactAction, Cli, Commands, ConfigAction, ExternalAction, HomeAction, PlatformsAction,
    SourceAction,
};
use cmx::context::AppContext;
use cmx::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use cmx::paths::ConfigPaths;
use cmx::platform::Platform;
use cmx::types::{ArtifactKind, InstallScope};

#[cfg(feature = "llm")]
use cmx::gateway::real::MojenticLlmClient;

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    // Path resolution needs one concrete "active" platform; absent `--platform`,
    // Claude is the canonical base (config_dir/home are platform-independent, so
    // this only affects single-platform commands like info/update/adopt).
    let active = cli.platform.unwrap_or(Platform::Claude);
    let paths = ConfigPaths::from_env(active)?;

    let ctx = AppContext {
        fs: &RealFilesystem,
        git: &RealGitClient,
        clock: &SystemClock,
        paths: &paths,
        llm: None,
    };

    run(cli, &ctx, &paths)
}

fn run(cli: Cli, ctx: &AppContext<'_>, paths: &ConfigPaths) -> Result<ExitCode> {
    let Cli {
        platform: selector,
        command,
    } = cli;
    match command {
        Commands::Source { action } => {
            handle_source(action, paths, ctx).map(|()| ExitCode::SUCCESS)
        }
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent, selector, ctx),
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill, selector, ctx),
        Commands::List { all } => {
            let output = cmx::list::list_all(all, ctx)?;
            print!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        Commands::Doctor {
            local,
            adopt_all,
            from,
            all,
        } => {
            if adopt_all {
                let outcome = cmx::adopt::adopt_all(None, from.as_deref(), local, ctx)?;
                print!("{outcome}");
                Ok(ExitCode::SUCCESS)
            } else if from.is_some() {
                bail!("--from only applies together with --adopt-all")
            } else {
                let mut report = cmx::doctor::survey(local, ctx)?;
                report.show_all = all;
                print!("{report}");
                if report.has_issues() {
                    Ok(ExitCode::from(2))
                } else {
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        Commands::Home { action } => handle_home(&action, ctx).map(|()| ExitCode::SUCCESS),
        Commands::Info { name } => handle_info(&name, None, ctx).map(|()| ExitCode::SUCCESS),
        Commands::Outdated => {
            cmx::source_update::ensure_fresh(ctx)?;
            let report = cmx::outdated::outdated(ctx)?;
            print!("{report}");
            Ok(ExitCode::SUCCESS)
        }
        Commands::Search { query } => {
            cmx::source_update::ensure_fresh(ctx)?;
            let output = cmx::search::search(&query, ctx)?;
            print!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        Commands::Config { action } => handle_config(action, ctx).map(|()| ExitCode::SUCCESS),
    }
}

fn handle_source(action: SourceAction, paths: &ConfigPaths, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        SourceAction::Add { name, path_or_url } => {
            if cmx::source::looks_like_url(&path_or_url) {
                let clone_dir = paths.git_clones_dir().join(&name);
                println!("Cloning {path_or_url} to {}...", clone_dir.display());
            }
            let result = cmx::source::add(&name, &path_or_url, ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::List => {
            let result = cmx::source::list(ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::Browse { name } => {
            let result = cmx::source::browse(&name, ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::Update { name } => {
            let output = cmx::source_update::update(name.as_deref(), ctx)?;
            print!("{output}");
            Ok(())
        }
        SourceAction::Remove { name } => {
            let result = cmx::source::remove(&name, ctx)?;
            print!("{result}");
            Ok(())
        }
    }
}

fn handle_home(action: &HomeAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        HomeAction::Init => {
            let home = cmx::adopt::home_init(ctx)?;
            println!("Canonical home ready at {}", home.display());
            println!("Registered as source '{}'.", cmx::adopt::HOME_SOURCE);
            Ok(())
        }
        HomeAction::Path => {
            let home = cmx::adopt::home_path(ctx)?;
            println!("{}", home.display());
            Ok(())
        }
    }
}

fn handle_config(action: ConfigAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let result = cmx::cmx_config::show(ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::Gateway { value } => {
            let result = cmx::cmx_config::set_gateway(&value, ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::Model { value } => {
            let result = cmx::cmx_config::set_model(&value, ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::External { action } => {
            let result = match action {
                ExternalAction::List => cmx::cmx_config::external_list(ctx)?,
                ExternalAction::Add { entry } => cmx::cmx_config::external_add(&entry, ctx)?,
                ExternalAction::Remove { entry } => cmx::cmx_config::external_remove(&entry, ctx)?,
            };
            print!("{result}");
            Ok(())
        }
        ConfigAction::Platforms { action } => {
            let result = match action {
                PlatformsAction::List => cmx::cmx_config::platforms_list(ctx)?,
                PlatformsAction::Add { platform } => cmx::cmx_config::platforms_add(platform, ctx)?,
                PlatformsAction::Remove { platform } => {
                    cmx::cmx_config::platforms_remove(platform, ctx)?
                }
            };
            print!("{result}");
            Ok(())
        }
    }
}

/// Show details for an installed artifact. `kind` is `Some` for the kind-scoped
/// `cmx {skill,agent} info`, `None` for the top-level `cmx info` (searches both).
/// In an `llm`-feature build with a configured gateway it also attaches a
/// generated "what it does" summary, best-effort — a generation failure leaves
/// the summary blank rather than failing the command.
fn handle_info(name: &str, kind: Option<ArtifactKind>, ctx: &AppContext<'_>) -> Result<()> {
    cmx::source_update::ensure_fresh(ctx)?;
    #[cfg_attr(not(feature = "llm"), allow(unused_mut))]
    let mut info = match kind {
        Some(k) => cmx::info::info_for_kind(name, k, ctx)?,
        None => cmx::info::info(name, ctx)?,
    };

    #[cfg(feature = "llm")]
    {
        let runner = build_llm_runtime(ctx)?;
        let llm_ctx = ctx.with_llm(&runner.llm);
        match runner.rt.block_on(cmx::info::summarize(&info, &llm_ctx)) {
            Ok(summary) => info.summary = Some(summary),
            // Best-effort: record *why* so the display reports the real reason
            // rather than always blaming the provider; never fail the command.
            Err(e) => info.summary_error = Some(cmx::info::condense_error(&e)),
        }
    }

    print!("{info}");
    Ok(())
}

/// Bundles the LLM client and a current-thread tokio runtime, extracted from
/// the config at `ctx`. Both sites that need LLM access (`handle_info` and the
/// `Diff` action) build the same boilerplate; this helper captures it once.
#[cfg(feature = "llm")]
struct LlmRuntime {
    llm: MojenticLlmClient,
    rt: tokio::runtime::Runtime,
}

#[cfg(feature = "llm")]
fn build_llm_runtime(ctx: &AppContext<'_>) -> Result<LlmRuntime> {
    let cfg = cmx::config::load_config(ctx.fs, ctx.paths)?;
    Ok(LlmRuntime {
        llm: MojenticLlmClient::new(cfg.llm),
        rt: tokio::runtime::Builder::new_current_thread().enable_all().build()?,
    })
}

fn handle_install(
    names: &[String],
    all: bool,
    local: bool,
    force: bool,
    kind: ArtifactKind,
    selector: Option<Platform>,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let scope = if local {
        InstallScope::Local
    } else {
        InstallScope::Global
    };
    cmx::source_update::ensure_fresh(ctx)?;
    let targets = cmx::install::resolve_targets(selector, kind, scope, ctx)?;
    if all {
        let result = cmx::install::install_all(kind, scope, force, &targets, ctx)?;
        print!("{result}");
        Ok(ExitCode::SUCCESS)
    } else if names.is_empty() {
        bail!("Provide artifact name(s) or use --all")
    } else {
        let result = cmx::install::install_many(names, kind, scope, force, &targets, ctx)?;
        let any_failed = !result.failed.is_empty();
        print!("{result}");
        Ok(if any_failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        })
    }
}

fn handle_artifact(
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
        } => handle_install(&names, all, local, force, kind, selector, ctx),
        ArtifactAction::List { all } => {
            let output = cmx::list::list_kind(kind, all, ctx)?;
            print!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Info { name } => {
            handle_info(&name, Some(kind), ctx).map(|()| ExitCode::SUCCESS)
        }
        #[cfg(feature = "llm")]
        ArtifactAction::Diff { name } => {
            cmx::source_update::ensure_fresh(ctx)?;
            let runner = build_llm_runtime(ctx)?;
            let diff_ctx = ctx.with_llm(&runner.llm);
            let output = runner.rt.block_on(cmx::diff::diff(&name, kind, &diff_ctx))?;
            print!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Update { name, all, force } => {
            cmx::source_update::ensure_fresh(ctx)?;
            if all {
                let result = cmx::install::update_all(kind, force, ctx)?;
                print!("{result}");
                Ok(ExitCode::SUCCESS)
            } else if let Some(name) = name {
                let result = cmx::install::update(&name, kind, force, ctx)?;
                print!("{result}");
                Ok(ExitCode::SUCCESS)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::Sync {
            name,
            from,
            dry_run,
            local,
        } => {
            let scope = if local {
                InstallScope::Local
            } else {
                InstallScope::Global
            };
            let result = cmx::sync::sync(&name, kind, scope, from, dry_run, ctx)?;
            print!("{result}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Promote { name } => {
            let result = cmx::promote::promote(&name, kind, ctx)?;
            print!("{result}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Uninstall { names, local } => {
            if names.is_empty() {
                bail!("Provide artifact name(s) to uninstall")
            }
            let scope = if local {
                InstallScope::Local
            } else {
                InstallScope::Global
            };
            let result = cmx::uninstall::uninstall_many(&names, kind, scope, selector, ctx)?;
            let none_removed = result.removed.is_empty();
            print!("{result}");
            // Exit non-zero only if nothing at all was removed (e.g. all typos).
            Ok(if none_removed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        ArtifactAction::Unadopt { names, external } => {
            handle_unadopt(&names, kind, external, ctx).map(|()| ExitCode::SUCCESS)
        }
        ArtifactAction::Adopt {
            names,
            all,
            from,
            local,
        } => {
            handle_adopt(&names, kind, all, from.as_deref(), local, ctx).map(|()| ExitCode::SUCCESS)
        }
    }
}

fn handle_unadopt(
    names: &[String],
    kind: ArtifactKind,
    external: bool,
    ctx: &AppContext<'_>,
) -> Result<()> {
    if names.is_empty() {
        bail!("Provide artifact name(s) to unadopt")
    }
    let outcome = cmx::adopt::unadopt_many(names, kind, ctx)?;
    print!("{outcome}");
    if external {
        for name in names {
            let r = cmx::cmx_config::external_add(name, ctx)?;
            print!("{r}");
        }
    }
    Ok(())
}

fn handle_adopt(
    names: &[String],
    kind: ArtifactKind,
    all: bool,
    from: Option<&std::path::Path>,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let outcome = if all {
        cmx::adopt::adopt_all(Some(kind), from, local, ctx)?
    } else if names.is_empty() {
        bail!("Provide artifact name(s) to adopt, or use --all")
    } else {
        cmx::adopt::adopt_named(kind, names, local, ctx)?
    };
    print!("{outcome}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cmx::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use cmx::platform::Platform;
    use std::path::PathBuf;

    fn test_paths() -> ConfigPaths {
        ConfigPaths::for_test(
            PathBuf::from("/home/testuser"),
            PathBuf::from("/home/testuser/.config/context-mixer"),
        )
    }

    fn make_test_ctx<'a>(
        fs: &'a FakeFilesystem,
        git: &'a FakeGitClient,
        clock: &'a FakeClock,
        paths: &'a ConfigPaths,
    ) -> AppContext<'a> {
        AppContext {
            fs,
            git,
            clock,
            paths,
            llm: None,
        }
    }

    fn fake_trio() -> (FakeFilesystem, FakeGitClient, FakeClock, ConfigPaths) {
        let paths = test_paths();
        (FakeFilesystem::new(), FakeGitClient::new(), FakeClock::at(Utc::now()), paths)
    }

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
        assert!(result.unwrap_err().to_string().contains("all"));
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
    }

    #[test]
    fn handle_unadopt_empty_names_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_unadopt(&[], ArtifactKind::Agent, false, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn handle_adopt_empty_names_no_all_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let result = handle_adopt(&[], ArtifactKind::Agent, false, None, false, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn handle_config_show_default_config_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_config(ConfigAction::Show, &ctx).is_ok());
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
        assert!(handle_home(&HomeAction::Path, &ctx).is_ok());
    }

    #[test]
    fn handle_home_init_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_home(&HomeAction::Init, &ctx).is_ok());
    }

    #[test]
    fn handle_source_list_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_source(SourceAction::List, &paths, &ctx).is_ok());
    }

    #[test]
    fn handle_info_unknown_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_info("nonexistent", None, &ctx).is_err());
    }

    #[test]
    fn run_list_all_returns_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::List { all: false },
        };
        assert!(run(cli, &ctx, &paths).is_ok());
    }

    #[test]
    fn run_outdated_returns_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Outdated,
        };
        assert!(run(cli, &ctx, &paths).is_ok());
    }

    fn setup_stale_git_source(
        fs: &cmx::gateway::fakes::FakeFilesystem,
        paths: &ConfigPaths,
        clone_path: &std::path::Path,
    ) {
        use cmx::types::{SourceEntry, SourceType, SourcesFile};
        use std::collections::BTreeMap;

        let old_time = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let mut sources = SourcesFile::default();
        sources.sources.insert(
            "stale-source".to_string(),
            SourceEntry {
                source_type: SourceType::Git,
                path: None,
                url: Some("https://github.com/example/repo.git".to_string()),
                local_clone: Some(clone_path.to_path_buf()),
                branch: Some("main".to_string()),
                last_updated: Some(old_time),
            },
        );
        let _ = BTreeMap::<String, SourceEntry>::new(); // unused, just to avoid unused import warning
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_dir(clone_path);
    }

    #[test]
    fn shell_refresh_happens_for_outdated_command() {
        let (fs, git, clock, paths) = fake_trio();
        let clone_path = std::path::PathBuf::from("/clones/stale-source");
        setup_stale_git_source(&fs, &paths, &clone_path);

        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Outdated,
        };
        run(cli, &ctx, &paths).unwrap();

        // Shell must have triggered exactly one pull for the stale source
        assert_eq!(
            git.pulled.borrow().len(),
            1,
            "expected shell to refresh stale source before outdated"
        );
    }

    #[test]
    fn outdated_core_does_not_pull_when_called_directly() {
        let (fs, git, clock, paths) = fake_trio();
        let clone_path = std::path::PathBuf::from("/clones/stale-source");
        setup_stale_git_source(&fs, &paths, &clone_path);

        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        // Call the core directly — no pull should happen
        cmx::outdated::outdated(&ctx).unwrap();

        assert!(
            git.pulled.borrow().is_empty(),
            "outdated core must not pull; refresh belongs to the shell"
        );
    }

    #[test]
    fn run_search_returns_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Search {
                query: "foo".to_string(),
            },
        };
        assert!(run(cli, &ctx, &paths).is_ok());
    }

    #[test]
    fn run_doctor_from_without_adopt_all_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Doctor {
                local: false,
                adopt_all: false,
                from: Some(PathBuf::from("/x")),
                all: false,
            },
        };
        assert!(run(cli, &ctx, &paths).is_err());
    }

    #[test]
    fn run_doctor_with_orphaned_artifact_returns_exit_code_2() {
        let (fs, git, clock, paths) = fake_trio();
        // Place an agent on disk with no lock entry — doctor classifies it as
        // Orphaned, which triggers has_issues(); previously process::exit(2),
        // now testable as Ok(ExitCode::from(2)).
        fs.add_file(
            "/home/testuser/.claude/agents/stray-agent.md",
            "---\nname: stray-agent\ndescription: a stray agent\n---\n# stray-agent\n",
        );
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Doctor {
                local: false,
                adopt_all: false,
                from: None,
                all: false,
            },
        };
        let result = run(cli, &ctx, &paths);
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::from(2));
    }
}
