use anyhow::{Result, bail};
use clap::Parser;

use cmx::cli::{
    ArtifactAction, Cli, Commands, ConfigAction, ExternalAction, HomeAction, SourceAction,
};
use cmx::context::AppContext;
use cmx::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use cmx::paths::ConfigPaths;
use cmx::types::{ArtifactKind, InstallScope};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = ConfigPaths::from_env(cli.platform)?;

    let ctx = AppContext {
        fs: &RealFilesystem,
        git: &RealGitClient,
        clock: &SystemClock,
        paths: &paths,
        llm: None,
    };

    match cli.command {
        Commands::Source { action } => handle_source(action, &paths, &ctx),
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent, &ctx),
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill, &ctx),
        Commands::List { all } => {
            let output = cmx::list::list_all(all, &ctx)?;
            print!("{output}");
            Ok(())
        }
        Commands::Doctor {
            local,
            adopt_all,
            from,
            all,
        } => {
            if adopt_all {
                let outcome = cmx::adopt::adopt_all(None, from.as_deref(), local, &ctx)?;
                print!("{outcome}");
                Ok(())
            } else if from.is_some() {
                bail!("--from only applies together with --adopt-all")
            } else {
                let mut report = cmx::doctor::survey(local, &ctx)?;
                report.show_all = all;
                print!("{report}");
                if report.has_issues() {
                    std::process::exit(2);
                }
                Ok(())
            }
        }
        Commands::Home { action } => handle_home(&action, &ctx),
        Commands::Info { name } => handle_info(&name, None, &ctx),
        Commands::Outdated => {
            let report = cmx::outdated::outdated(&ctx)?;
            print!("{report}");
            Ok(())
        }
        Commands::Search { query } => {
            let output = cmx::search::search(&query, &ctx)?;
            print!("{output}");
            Ok(())
        }
        Commands::Config { action } => handle_config(action, &ctx),
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
    }
}

/// Show details for an installed artifact. `kind` is `Some` for the kind-scoped
/// `cmx {skill,agent} info`, `None` for the top-level `cmx info` (searches both).
/// In an `llm`-feature build with a configured gateway it also attaches a
/// generated "what it does" summary, best-effort — a generation failure leaves
/// the summary blank rather than failing the command.
fn handle_info(name: &str, kind: Option<ArtifactKind>, ctx: &AppContext<'_>) -> Result<()> {
    #[cfg_attr(not(feature = "llm"), allow(unused_mut))]
    let mut info = match kind {
        Some(k) => cmx::info::info_for_kind(name, k, ctx)?,
        None => cmx::info::info(name, ctx)?,
    };

    #[cfg(feature = "llm")]
    {
        use cmx::gateway::real::MojenticLlmClient;
        let cfg = cmx::config::load_config(ctx.fs, ctx.paths)?;
        let llm = MojenticLlmClient::new(cfg.llm);
        let llm_ctx = AppContext {
            fs: ctx.fs,
            git: ctx.git,
            clock: ctx.clock,
            paths: ctx.paths,
            llm: Some(&llm),
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        match rt.block_on(cmx::info::summarize(&info, &llm_ctx)) {
            Ok(summary) => info.summary = Some(summary),
            // Best-effort: record *why* so the display reports the real reason
            // rather than always blaming the provider; never fail the command.
            Err(e) => info.summary_error = Some(condense_error(&e)),
        }
    }

    print!("{info}");
    Ok(())
}

/// Render an error as a single, length-capped line for the one-line "What it
/// does" reason. `{:#}` gives anyhow's full context chain (so the underlying
/// provider/credential cause survives), then we flatten whitespace — provider
/// errors often embed a multi-line JSON body — and truncate.
#[cfg(feature = "llm")]
fn condense_error(e: &anyhow::Error) -> String {
    const MAX: usize = 200;
    let flattened = format!("{e:#}").split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() > MAX {
        let head: String = flattened.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        flattened
    }
}

fn handle_artifact(action: ArtifactAction, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ArtifactAction::Install {
            names,
            all,
            local,
            force,
        } => {
            let scope = if local {
                InstallScope::Local
            } else {
                InstallScope::Global
            };
            if all {
                let result = cmx::install::install_all(kind, scope, force, ctx)?;
                print!("{result}");
                Ok(())
            } else if names.is_empty() {
                bail!("Provide artifact name(s) or use --all")
            } else {
                let result = cmx::install::install_many(&names, kind, scope, force, ctx)?;
                let any_failed = !result.failed.is_empty();
                print!("{result}");
                if any_failed {
                    std::process::exit(1);
                }
                Ok(())
            }
        }
        ArtifactAction::List { all } => {
            let output = cmx::list::list_kind(kind, all, ctx)?;
            print!("{output}");
            Ok(())
        }
        ArtifactAction::Info { name } => handle_info(&name, Some(kind), ctx),
        #[cfg(feature = "llm")]
        ArtifactAction::Diff { name } => {
            use cmx::gateway::real::MojenticLlmClient;
            let cfg = cmx::config::load_config(ctx.fs, ctx.paths)?;
            let llm = MojenticLlmClient::new(cfg.llm);
            let diff_ctx = AppContext {
                fs: ctx.fs,
                git: ctx.git,
                clock: ctx.clock,
                paths: ctx.paths,
                llm: Some(&llm),
            };
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            let output = rt.block_on(cmx::diff::diff(&name, kind, &diff_ctx))?;
            print!("{output}");
            Ok(())
        }
        ArtifactAction::Update { name, all, force } => {
            if all {
                let result = cmx::install::update_all(kind, force, ctx)?;
                print!("{result}");
                Ok(())
            } else if let Some(name) = name {
                let result = cmx::install::update(&name, kind, force, ctx)?;
                print!("{result}");
                Ok(())
            } else {
                bail!("Provide an artifact name or use --all")
            }
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
            let result = cmx::uninstall::uninstall_many(&names, kind, scope, ctx)?;
            let none_removed = result.removed.is_empty();
            print!("{result}");
            // Exit non-zero only if nothing at all was removed (e.g. all typos).
            if none_removed {
                std::process::exit(1);
            }
            Ok(())
        }
        ArtifactAction::Unadopt { names, external } => handle_unadopt(&names, kind, external, ctx),
        ArtifactAction::Adopt {
            names,
            all,
            from,
            local,
        } => handle_adopt(&names, kind, all, from.as_deref(), local, ctx),
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
