use anyhow::{Result, bail};
use clap::Parser;
use serde::Serialize;
use std::process::ExitCode;

use cmx::cli::{
    ArtifactAction, Cli, Commands, ConfigAction, ExternalAction, HomeAction, OutputArgs,
    PlatformsAction, SetAction, SourceAction,
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
        Commands::Set { action } => handle_set(action, ctx),
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent, selector, ctx),
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill, selector, ctx),
        Commands::List { all, output } => {
            let report = cmx::list::list_all(all, ctx)?;
            if output.json {
                print_json(&cmx::display::json::list_json(&report))?;
            } else {
                print!("{report}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Doctor {
            local,
            adopt_all,
            from,
            all,
            output,
        } => {
            if adopt_all {
                eprintln!("{}", cmx::display::doctor::adopt_all_deprecation_notice());
                let outcome = cmx::adopt::adopt_all(None, from.as_deref(), local, ctx)?;
                if output.json {
                    let mut report = cmx::doctor::survey(local, ctx)?;
                    report.show_all = all;
                    print_json(&cmx::display::doctor::doctor_json(&report))?;
                } else {
                    print!("{outcome}");
                }
                Ok(ExitCode::SUCCESS)
            } else if from.is_some() {
                bail!("--from only applies together with --adopt-all")
            } else {
                let mut report = cmx::doctor::survey(local, ctx)?;
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
        Commands::Home { action } => handle_home(&action, ctx).map(|()| ExitCode::SUCCESS),
        Commands::Info { name, output } => {
            handle_info(&name, None, output.json, ctx).map(|()| ExitCode::SUCCESS)
        }
        Commands::Outdated { output } => {
            cmx::source_update::ensure_fresh(ctx)?;
            let report = cmx::outdated::outdated(ctx)?;
            if output.json {
                print_json(&cmx::display::json::outdated_json(&report))?;
            } else {
                print!("{report}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Search { query, output } => {
            cmx::source_update::ensure_fresh(ctx)?;
            let results = cmx::search::search(&query, ctx)?;
            if output.json {
                print_json(&cmx::display::json::search_json(&results))?;
            } else {
                print!("{results}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Config { action } => handle_config(action, ctx).map(|()| ExitCode::SUCCESS),
        Commands::Init {
            local,
            global: _,
            force,
            remove,
            output,
        } => handle_init(
            InitArgs {
                local,
                force,
                remove,
                output,
            },
            ctx,
        ),
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Flags for `cmx init`, grouped to keep `handle_init`'s signature under
/// clippy's excessive-bool-parameters threshold. `--global` is destructured-
/// and-ignored in `run` above: a no-op alias for one release, since global is
/// already the default scope.
///
/// Four independent flags mirroring the clap `Init` variant 1:1 — not a state
/// machine, so `clippy::struct_excessive_bools` is silenced deliberately here.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct InitArgs {
    local: bool,
    force: bool,
    remove: bool,
    output: OutputArgs,
}

/// `cmx init` — install/remove cmx's own companion skill via cmx-core.
fn handle_init(args: InitArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let outcome = if args.remove {
        cmx::init::run_remove(args.local, ctx)?
    } else {
        cmx::init::run_init(args.local, args.force, ctx)?
    };
    if args.output.json {
        print_json(&cmx::display::init::init_json(&outcome))?;
    } else {
        print!("{outcome}");
    }
    Ok(outcome.exit_code())
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
        SourceAction::List { output } => {
            let result = cmx::source::list(ctx)?;
            if output.json {
                print_json(&cmx::display::json::source_list_json(&result))?;
            } else {
                print!("{result}");
            }
            Ok(())
        }
        SourceAction::Browse { name, output } => {
            let result = cmx::source::browse(&name, ctx)?;
            if output.json {
                print_json(&cmx::display::json::source_browse_json(&result))?;
            } else {
                print!("{result}");
            }
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

fn handle_set(action: SetAction, ctx: &AppContext<'_>) -> Result<ExitCode> {
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
            dry_run,
            local,
        } => handle_set_activate(&name, dry_run, local, ctx),
        SetAction::Deactivate {
            name,
            dry_run,
            force,
            local,
        } => handle_set_deactivate(&name, dry_run, force, local, ctx),
        SetAction::Delete {
            name,
            local,
            purge,
            force,
        } => handle_set_delete(&name, local, purge, force, ctx),
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
    let result = cmx::sets::create(name, desc, from, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_list(local: bool, output: OutputArgs, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let scope = scope_from(local);
    let result = cmx::sets::list(scope, ctx)?;
    if output.json {
        print_json(&cmx::display::json::set_list_json(&result, scope))?;
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
    let result = cmx::sets::show(name, scope, ctx)?;
    if output.json {
        print_json(&cmx::display::json::set_show_json(&result, scope))?;
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
    let result = cmx::sets::add(name, artifacts, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_remove(
    name: &str,
    artifacts: &[String],
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = cmx::sets::remove(name, artifacts, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_activate(
    name: &str,
    dry_run: bool,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = cmx::sets::activate(name, dry_run, scope_from(local), ctx)?;
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
    dry_run: bool,
    force: bool,
    local: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = cmx::sets::deactivate(name, force, dry_run, scope_from(local), ctx)?;
    let any_blocked = result.any_blocked;
    print!("{result}");
    Ok(if any_blocked {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn handle_set_delete(
    name: &str,
    local: bool,
    purge: bool,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    let result = cmx::sets::delete(name, purge, force, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn handle_set_rename(old: &str, new: &str, local: bool, ctx: &AppContext<'_>) -> Result<ExitCode> {
    let result = cmx::sets::rename(old, new, scope_from(local), ctx)?;
    print!("{result}");
    Ok(ExitCode::SUCCESS)
}

fn scope_from(local: bool) -> InstallScope {
    if local {
        InstallScope::Local
    } else {
        InstallScope::Global
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
        HomeAction::Path { output } => {
            let home = cmx::adopt::home_path(ctx)?;
            if output.json {
                print_json(&cmx::display::json::home_path_json(&home))?;
            } else {
                println!("{}", home.display());
            }
            Ok(())
        }
    }
}

fn handle_config(action: ConfigAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ConfigAction::Show { output } => {
            let result = cmx::cmx_config::show(ctx)?;
            if output.json {
                print_json(&cmx::display::json::config_show_json(&result))?;
            } else {
                print!("{result}");
            }
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
fn handle_info(
    name: &str,
    kind: Option<ArtifactKind>,
    json_output: bool,
    ctx: &AppContext<'_>,
) -> Result<()> {
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

    if json_output {
        print_json(&cmx::display::json::info_json(&info))?;
    } else {
        print!("{info}");
    }
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

/// `cmx {agent,skill} diff` — the structural diff always runs (no LLM
/// involved on `--full`); compact mode additionally attempts an LLM summary,
/// degrading to a one-line note on any failure (unconfigured gateway, auth
/// error, network error, or — in a lean build — no `llm` feature at all).
/// Only a genuine diff-compute error (artifact not found, unreadable files)
/// propagates as an `Err`.
#[cfg(feature = "llm")]
fn handle_diff(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<cmx::diff::DiffOutput> {
    match build_llm_runtime(ctx) {
        Ok(runner) => {
            let diff_ctx = ctx.with_llm(&runner.llm);
            runner.rt.block_on(cmx::diff::diff_with_analysis(name, kind, full, &diff_ctx))
        }
        Err(e) => {
            let mut output = cmx::diff::diff(name, kind, full, ctx)?;
            if !output.show_full && !output.is_up_to_date {
                output.analysis_note = Some(cmx::diff::llm_unavailable_note(&e));
            }
            Ok(output)
        }
    }
}

#[cfg(not(feature = "llm"))]
fn handle_diff(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<cmx::diff::DiffOutput> {
    let mut output = cmx::diff::diff(name, kind, full, ctx)?;
    if !output.show_full && !output.is_up_to_date {
        output.analysis_note = Some(cmx::diff::llm_lean_note());
    }
    Ok(output)
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
        ArtifactAction::List { all, output } => {
            let report = cmx::list::list_kind(kind, all, ctx)?;
            if output.json {
                print_json(&cmx::display::json::list_kind_json(&report))?;
            } else {
                print!("{report}");
            }
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Info { name, output } => {
            handle_info(&name, Some(kind), output.json, ctx).map(|()| ExitCode::SUCCESS)
        }
        ArtifactAction::Diff { name, full } => {
            cmx::source_update::ensure_fresh(ctx)?;
            let output = handle_diff(&name, kind, full, ctx)?;
            print!("{output}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Update { name, all, force } => handle_update(name, all, force, kind, ctx),
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
        ArtifactAction::Promote { name, from } => {
            let result = cmx::promote::promote(&name, kind, from.or(selector), ctx)?;
            print!("{result}");
            Ok(ExitCode::SUCCESS)
        }
        ArtifactAction::Uninstall { names, local } => {
            handle_uninstall(&names, local, kind, selector, ctx)
        }
        ArtifactAction::Unadopt { names, external } => {
            handle_unadopt(&names, kind, external, ctx).map(|()| ExitCode::SUCCESS)
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
            handle_adopt(
                &names,
                kind,
                all,
                from_dir.as_deref().or(deprecated_from.as_deref()),
                local,
                ctx,
            )
            .map(|()| ExitCode::SUCCESS)
        }
    }
}

fn handle_update(
    name: Option<String>,
    all: bool,
    force: bool,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
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

fn handle_uninstall(
    names: &[String],
    local: bool,
    kind: ArtifactKind,
    selector: Option<Platform>,
    ctx: &AppContext<'_>,
) -> Result<ExitCode> {
    if names.is_empty() {
        bail!("Provide artifact name(s) to uninstall")
    }
    let scope = if local {
        InstallScope::Local
    } else {
        InstallScope::Global
    };
    let result = cmx::uninstall::uninstall_many(names, kind, scope, selector, ctx)?;
    let none_removed = result.removed.is_empty();
    print!("{result}");
    Ok(if none_removed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
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
    use cmx::gateway::Filesystem;
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

    fn no_json() -> OutputArgs {
        OutputArgs { json: false }
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

    #[test]
    fn handle_source_list_ok() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_source(SourceAction::List { output: no_json() }, &paths, &ctx).is_ok());
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
        cmx_core::test_support::setup_source_with_agent(
            &fs,
            &paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
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
        cmx::config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| {
            sets.sets.get_mut("rust-work").unwrap().members.push(cmx::types::SetMember {
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
                force: false,
            },
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
        let sets = cmx::config::load_sets(InstallScope::Global, &fs, &paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
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
        cmx::config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| {
            sets.sets.get_mut("rust-work").unwrap().members.push(cmx::types::SetMember {
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
        cmx_core::test_support::setup_source_with_agent(
            &fs,
            &paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
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
        cmx::config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| {
            sets.sets.get_mut("rust-work").unwrap().members.push(cmx::types::SetMember {
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
                dry_run: true,
                local: false,
            },
            &ctx,
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
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
                cmx::checksum::checksum_artifact(skill_path, ArtifactKind::Skill, &fs).unwrap();
            let pv = paths.with_platform(platform);
            cmx::lockfile::mutate(InstallScope::Global, &fs, &pv, |lock| {
                lock.packages.insert(
                    "pf".to_string(),
                    cmx::types::LockEntry {
                        artifact_type: ArtifactKind::Skill,
                        version: Some("1.0.0".to_string()),
                        installed_at: "2026-07-05T00:00:00Z".to_string(),
                        source: cmx::types::LockSource {
                            repo: cmx::adopt::HOME_SOURCE.to_string(),
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

    #[test]
    fn handle_info_unknown_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_info("nonexistent", None, false, &ctx).is_err());
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
        cmx::config::mutate_sets(InstallScope::Global, &fs, &paths, |sets| {
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
        })
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
            local: false,
            force: false,
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
            local: false,
            force: false,
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
            local: false,
            force: false,
            remove: false,
            output: no_json(),
        };
        assert!(handle_init(install, &ctx).is_ok());
        let remove = InitArgs {
            local: false,
            force: false,
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
            local: false,
            force: true,
            remove: false,
            output: no_json(),
        };
        let result = handle_init(args, &ctx);
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}
