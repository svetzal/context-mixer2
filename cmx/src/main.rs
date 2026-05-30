use anyhow::{Result, bail};
use clap::Parser;

use cmx::cli::{ArtifactAction, Cli, Commands, ConfigAction, SourceAction};
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
        Commands::List => {
            let output = cmx::list::list_all(&ctx)?;
            print!("{output}");
            Ok(())
        }
        Commands::Doctor { local } => {
            let report = cmx::doctor::survey(local, &ctx)?;
            print!("{report}");
            if report.has_issues() {
                std::process::exit(2);
            }
            Ok(())
        }
        Commands::Info { name } => {
            let info = cmx::info::info(&name, &ctx)?;
            print!("{info}");
            Ok(())
        }
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
    }
}

fn handle_artifact(action: ArtifactAction, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ArtifactAction::Install {
            name,
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
            } else if let Some(name) = name {
                let result = cmx::install::install(&name, kind, scope, force, ctx)?;
                print!("{result}");
                Ok(())
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::List => {
            let output = cmx::list::list_kind(kind, ctx)?;
            print!("{output}");
            Ok(())
        }
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
        ArtifactAction::Uninstall { name, local } => {
            let scope = if local {
                InstallScope::Local
            } else {
                InstallScope::Global
            };
            let result = cmx::uninstall::uninstall(&name, kind, scope, ctx)?;
            print!("{result}");
            Ok(())
        }
    }
}
