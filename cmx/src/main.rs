//! Binary entry point for the `cmx` CLI: constructs an `AppContext` with real
//! gateways and dispatches parsed `Commands` to the `cmx::dispatch` handlers.

use anyhow::{Result, bail};
use clap::Parser;
use serde::Serialize;
use std::io;
use std::process::ExitCode;

use cmx::cli::{Cli, Commands, OutputArgs};
use cmx::context::AppContext;
use cmx::dispatch::{
    handle_artifact, handle_config, handle_home, handle_info, handle_set, handle_source, scope_from,
};
use cmx::flags::{Force, SurveyScope};
use cmx::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use cmx::paths::ConfigPaths;
use cmx::platform::Platform;
use cmx::types::{ArtifactKind, InstallScope};

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    if let Commands::Completions { shell } = &cli.command {
        return handle_completions(*shell);
    }

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
        Commands::Set { action } => handle_set(action, ctx),
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent, selector, ctx),
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill, selector, ctx),
        Commands::List { all, output } => handle_list(all, output, ctx),
        Commands::Doctor {
            local,
            adopt_all,
            from,
            all,
            output,
        } => handle_doctor(
            SurveyScope::from_flag(local),
            adopt_all,
            from.as_deref(),
            all,
            output,
            ctx,
        ),
        Commands::Home { action } => handle_home(&action, ctx).map(|()| ExitCode::SUCCESS),
        Commands::Info { name, output } => {
            handle_info(&name, None, output.json, ctx).map(|()| ExitCode::SUCCESS)
        }
        Commands::Completions { shell } => handle_completions(shell),
        Commands::Outdated { output } => handle_outdated(output, ctx),
        Commands::Search { query, output } => handle_search(&query, output, ctx),
        Commands::Config { action } => handle_config(action, ctx).map(|()| ExitCode::SUCCESS),
        Commands::Init {
            local,
            global: _,
            force,
            remove,
            output,
        } => handle_init(
            InitArgs {
                scope: scope_from(local),
                force: Force::from_flag(force),
                remove,
                output,
            },
            ctx,
        ),
    }
}

fn handle_list(all: bool, output: OutputArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let report = cmx::list::list_all(all, ctx)?;
    if output.json {
        print_json(&cmx::display::json::list_json(&report))?;
    } else {
        print!("{report}");
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_doctor(
    scope: SurveyScope,
    adopt_all: bool,
    from: Option<&std::path::Path>,
    all: bool,
    output: OutputArgs,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    if adopt_all {
        eprintln!("{}", cmx::display::doctor::adopt_all_deprecation_notice());
        let outcome = cmx::adopt::adopt_all(None, from, scope, ctx)?;
        if output.json {
            let mut report = cmx::doctor::survey(scope, ctx)?;
            report.show_all = all;
            print_json(&cmx::display::doctor::doctor_json(&report))?;
        } else {
            print!("{outcome}");
        }
        Ok(ExitCode::SUCCESS)
    } else if from.is_some() {
        bail!("--from only applies together with --adopt-all")
    } else {
        let mut report = cmx::doctor::survey(scope, ctx)?;
        report.show_all = all;
        if output.json {
            print_json(&cmx::display::doctor::doctor_json(&report))?;
        } else {
            print!("{report}");
        }
        if report.has_issues() {
            Ok(ExitCode::from(2))
        } else {
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn handle_outdated(output: OutputArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    cmx::source_update::ensure_fresh(ctx)?;
    let report = cmx::outdated::outdated(ctx)?;
    if output.json {
        print_json(&cmx::display::json::outdated_json(&report))?;
    } else {
        print!("{report}");
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_search(query: &str, output: OutputArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    cmx::source_update::ensure_fresh(ctx)?;
    let results = cmx::search::search(query, ctx)?;
    if output.json {
        print_json(&cmx::display::json::search_json(&results))?;
    } else {
        print!("{results}");
    }
    Ok(ExitCode::SUCCESS)
}

fn handle_completions(shell: clap_complete::Shell) -> Result<ExitCode> {
    let mut stdout = io::stdout().lock();
    cmx::completions::generate_to(shell, &mut stdout)?;
    Ok(ExitCode::SUCCESS)
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Flags for `cmx init`, grouped to keep `handle_init`'s signature focused.
/// `--global` is destructured-and-ignored in `run` above: a no-op alias for
/// one release, since global is already the default scope.
#[derive(Clone, Copy)]
struct InitArgs {
    scope: InstallScope,
    force: Force,
    remove: bool,
    output: OutputArgs,
}

/// `cmx init` — install/remove cmx's own companion skill via cmx-core.
fn handle_init(args: InitArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let scope = match args.scope {
        InstallScope::Local => cmx_core::skill_install::Scope::Local,
        InstallScope::Global => cmx_core::skill_install::Scope::Global,
    };
    let outcome = if args.remove {
        cmx::init::run_remove(scope, ctx)?
    } else {
        cmx::init::run_init(scope, args.force, ctx)?
    };
    if args.output.json {
        print_json(&cmx::display::init::init_json(&outcome))?;
    } else {
        print!("{outcome}");
    }
    Ok(outcome.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use cmx::cli::{Commands, OutputArgs};
    use cmx::flags::Force;
    use cmx::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use cmx::platform::Platform;
    use cmx::types::{ArtifactKind, InstallScope};
    use cmx_core::test_support::metadata_versioned_skill_content;
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
    ) -> cmx::context::AppContext<'a> {
        cmx::context::AppContext {
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

    fn no_json() -> OutputArgs {
        OutputArgs { json: false }
    }

    #[test]
    fn run_list_all_returns_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::List {
                all: false,
                output: no_json(),
            },
        };
        assert!(run(cli, &ctx, &paths).is_ok());
    }

    #[test]
    fn run_outdated_returns_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Outdated { output: no_json() },
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
            command: Commands::Outdated { output: no_json() },
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
                output: no_json(),
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
                output: no_json(),
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
                output: no_json(),
            },
        };
        let result = run(cli, &ctx, &paths);
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::from(2));
    }

    #[test]
    fn run_doctor_with_set_inconsistency_returns_exit_code_2() {
        // An active set whose member was never installed (or was manually
        // uninstalled) is a set/installed-state mismatch — Phase 3 of SETS.md
        // wires this into doctor's existing exit-code-2 contract.
        let (fs, git, clock, paths) = fake_trio();
        cmx::config::mutate_sets(
            InstallScope::Global,
            &fs,
            &paths,
            |sets| -> cmx::error::Result<()> {
                sets.sets.insert(
                    "rust-work".to_string(),
                    cmx::types::SetDef {
                        description: None,
                        state: cmx::types::SetState::Active,
                        members: vec![cmx::types::SetMember {
                            kind: ArtifactKind::Agent,
                            name: "rust-craftsperson".to_string(),
                            source: Some("guidelines".to_string()),
                        }],
                    },
                );
                Ok(())
            },
        )
        .unwrap();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Doctor {
                local: false,
                adopt_all: false,
                from: None,
                all: false,
                output: no_json(),
            },
        };
        let result = run(cli, &ctx, &paths);
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::from(2));
    }

    #[test]
    fn run_doctor_clean_config_with_no_sets_returns_success() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Doctor {
                local: false,
                adopt_all: false,
                from: None,
                all: false,
                output: no_json(),
            },
        };
        let result = run(cli, &ctx, &paths);
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn run_doctor_adopt_all_still_adopts_despite_deprecation() {
        // `--adopt-all` is soft-deprecated (prints a notice to stderr via
        // `adopt_all_deprecation_notice`) but must keep working this release.
        let (fs, git, clock, paths) = fake_trio();
        fs.add_file(
            "/home/testuser/.claude/agents/stray-agent.md",
            "---\nname: stray-agent\ndescription: a stray agent\n---\n# stray-agent\n",
        );
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let cli = Cli {
            platform: Some(Platform::Claude),
            command: Commands::Doctor {
                local: false,
                adopt_all: true,
                from: None,
                all: false,
                output: no_json(),
            },
        };
        let result = run(cli, &ctx, &paths);
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn handle_init_installs_and_exits_success() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let args = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: false,
            output: no_json(),
        };
        let result = handle_init(args, &ctx);
        assert!(result.is_ok(), "expected Ok, not Err: {:?}", result.err());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn handle_init_json_flag_exits_success() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let args = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: false,
            output: OutputArgs { json: true },
        };
        let result = handle_init(args, &ctx);
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn handle_init_remove_after_install_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let install = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: false,
            output: no_json(),
        };
        assert!(handle_init(install, &ctx).is_ok());
        let remove = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: true,
            output: no_json(),
        };
        let result = handle_init(remove, &ctx);
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn handle_init_global_force_exits_success() {
        // Mirrors the fleet-wide registry contract: `<tool> init --global --force`
        // must exit 0 (foundry's registry derives this invocation for
        // skill-installing tools). `--global` is a no-op alias for `global`;
        // `handle_init` never receives it (destructured-and-ignored in `run`).
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let args = InitArgs {
            scope: InstallScope::Global,
            force: Force::Yes,
            remove: false,
            output: no_json(),
        };
        let result = handle_init(args, &ctx);
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn handle_init_drifted_copy_without_force_returns_failure() {
        let (fs, git, clock, paths) = fake_trio();
        let initial_ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let initial_args = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: false,
            output: no_json(),
        };
        assert_eq!(handle_init(initial_args, &initial_ctx).unwrap(), ExitCode::SUCCESS);

        let pv = paths.with_platform(Platform::Claude);
        let skill_dir =
            pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap().join("cmx");
        fs.add_file(
            skill_dir.join("SKILL.md"),
            metadata_versioned_skill_content("locally edited", env!("CARGO_PKG_VERSION")),
        );

        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let args = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: false,
            output: no_json(),
        };
        let result = handle_init(args, &ctx);
        assert_eq!(result.unwrap(), ExitCode::FAILURE);
    }

    #[test]
    fn handle_init_up_to_date_rerun_remains_success() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        let args = InitArgs {
            scope: InstallScope::Global,
            force: Force::No,
            remove: false,
            output: no_json(),
        };

        assert_eq!(handle_init(args, &ctx).unwrap(), ExitCode::SUCCESS);
        assert_eq!(handle_init(args, &ctx).unwrap(), ExitCode::SUCCESS);
    }
}
