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
            let output = cmx::list::list_all_with(&ctx)?;
            print!("{output}");
            Ok(())
        }
        Commands::Info { name } => {
            let info = cmx::info::info_with(&name, &ctx)?;
            print!("{info}");
            Ok(())
        }
        Commands::Outdated => {
            let report = cmx::outdated::outdated_with(&ctx)?;
            print!("{report}");
            Ok(())
        }
        Commands::Search { query } => {
            let output = cmx::search::search_with(&query, &ctx)?;
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
            let result = cmx::source::add_with(&name, &path_or_url, ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::List => {
            let result = cmx::source::list_with(ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::Browse { name } => {
            let result = cmx::source::browse_with(&name, ctx)?;
            print!("{result}");
            Ok(())
        }
        SourceAction::Update { name } => {
            let output = cmx::source_update::update_with(name.as_deref(), ctx)?;
            print!("{output}");
            Ok(())
        }
        SourceAction::Remove { name } => {
            let result = cmx::source::remove_with(&name, ctx)?;
            print!("{result}");
            Ok(())
        }
    }
}

fn handle_config(action: ConfigAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let result = cmx::cmx_config::show_with(ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::Gateway { value } => {
            let result = cmx::cmx_config::set_gateway_with(&value, ctx)?;
            print!("{result}");
            Ok(())
        }
        ConfigAction::Model { value } => {
            let result = cmx::cmx_config::set_model_with(&value, ctx)?;
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
                let result = cmx::install::install_all_with(kind, scope, force, ctx)?;
                print!("{result}");
                Ok(())
            } else if let Some(name) = name {
                let result = cmx::install::install_with(&name, kind, scope, force, ctx)?;
                print!("{result}");
                Ok(())
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::List => {
            let output = cmx::list::list_kind_with(kind, ctx)?;
            print!("{output}");
            Ok(())
        }
        #[cfg(feature = "llm")]
        ArtifactAction::Diff { name } => {
            use cmx::gateway::real::MojenticLlmClient;
            let cfg = cmx::config::load_config_with(ctx.fs, ctx.paths)?;
            let llm = MojenticLlmClient::new(cfg.llm);
            let diff_ctx = AppContext {
                fs: ctx.fs,
                git: ctx.git,
                clock: ctx.clock,
                paths: ctx.paths,
                llm: Some(&llm),
            };
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            let output = rt.block_on(cmx::diff::diff_with(&name, kind, &diff_ctx))?;
            print!("{output}");
            Ok(())
        }
        ArtifactAction::Update { name, all, force } => {
            if all {
                let result = cmx::install::update_all_with(kind, force, ctx)?;
                print!("{result}");
                Ok(())
            } else if let Some(name) = name {
                let result = cmx::install::update_with(&name, kind, force, ctx)?;
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
            let result = cmx::uninstall::uninstall_with(&name, kind, scope, ctx)?;
            print!("{result}");
            Ok(())
        }
    }
}
