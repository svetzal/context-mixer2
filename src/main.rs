use anyhow::{Result, bail};
use clap::Parser;

use cmx::cli::{ArtifactAction, Cli, Commands, ConfigAction, SourceAction};
use cmx::context::AppContext;
use cmx::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use cmx::paths::ConfigPaths;
use cmx::types::ArtifactKind;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = ConfigPaths::from_env()?;

    let ctx = AppContext {
        fs: &RealFilesystem,
        git: &RealGitClient,
        clock: &SystemClock,
        paths: &paths,
        llm: None,
    };

    match cli.command {
        Commands::Source { action } => match action {
            SourceAction::Add { name, path_or_url } => {
                cmx::source::add_with(&name, &path_or_url, &ctx)
            }
            SourceAction::List => cmx::source::list_with(&ctx),
            SourceAction::Browse { name } => cmx::source::browse_with(&name, &ctx),
            SourceAction::Update { name } => cmx::source::update_with(name.as_deref(), &ctx),
            SourceAction::Remove { name } => cmx::source::remove_with(&name, &ctx),
        },
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent, &ctx).await,
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill, &ctx).await,
        Commands::List => cmx::list::list_all_with(&ctx),
        Commands::Info { name } => cmx::info::info_with(&name, &ctx),
        Commands::Outdated => cmx::outdated::outdated_with(&ctx),
        Commands::Search { query } => cmx::search::search_with(&query, &ctx),
        Commands::Config { action } => match action {
            ConfigAction::Show => cmx::cmx_config::show_with(&ctx),
            ConfigAction::Gateway { value } => cmx::cmx_config::set_gateway_with(&value, &ctx),
            ConfigAction::Model { value } => cmx::cmx_config::set_model_with(&value, &ctx),
        },
    }
}

async fn handle_artifact(
    action: ArtifactAction,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<()> {
    match action {
        ArtifactAction::Install {
            name,
            all,
            local,
            force,
        } => {
            if all {
                cmx::install::install_all_with(kind, local, force, ctx)
            } else if let Some(name) = name {
                cmx::install::install_with(&name, kind, local, force, ctx)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::List => cmx::list::list_kind_with(kind, ctx),
        ArtifactAction::Diff { name } => {
            // Diff needs the LLM client — construct it from config
            let cfg = cmx::config::load_config_with(ctx.fs, ctx.paths)?;
            let llm = cmx::gateway::real::MojenticLlmClient::new(cfg.llm);
            let diff_ctx = AppContext {
                fs: ctx.fs,
                git: ctx.git,
                clock: ctx.clock,
                paths: ctx.paths,
                llm: Some(&llm),
            };
            cmx::diff::diff_with(&name, kind, &diff_ctx).await
        }
        ArtifactAction::Update { name, all, force } => {
            if all {
                cmx::install::update_all_with(kind, force, ctx)
            } else if let Some(name) = name {
                cmx::install::update_with(&name, kind, force, ctx)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::Uninstall { name, local } => {
            cmx::uninstall::uninstall_with(&name, kind, local, ctx)
        }
    }
}
