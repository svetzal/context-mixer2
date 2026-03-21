use anyhow::{Result, bail};
use clap::Parser;

use cmx::cli::{ArtifactAction, Cli, Commands, ConfigAction, SourceAction};
use cmx::types::ArtifactKind;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Source { action } => match action {
            SourceAction::Add { name, path_or_url } => cmx::source::add(&name, &path_or_url),
            SourceAction::List => cmx::source::list(),
            SourceAction::Browse { name } => cmx::source::browse(&name),
            SourceAction::Update { name } => cmx::source::update(name.as_deref()),
            SourceAction::Remove { name } => cmx::source::remove(&name),
        },
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent).await,
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill).await,
        Commands::List => cmx::list::list_all(),
        Commands::Info { name } => cmx::info::info(&name),
        Commands::Outdated => cmx::outdated::outdated(),
        Commands::Search { query } => cmx::search::search(&query),
        Commands::Config { action } => match action {
            ConfigAction::Show => cmx::cmx_config::show(),
            ConfigAction::Gateway { value } => cmx::cmx_config::set_gateway(&value),
            ConfigAction::Model { value } => cmx::cmx_config::set_model(&value),
        },
    }
}

async fn handle_artifact(action: ArtifactAction, kind: ArtifactKind) -> Result<()> {
    match action {
        ArtifactAction::Install {
            name,
            all,
            local,
            force,
        } => {
            if all {
                cmx::install::install_all(kind, local, force)
            } else if let Some(name) = name {
                cmx::install::install(&name, kind, local, force)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::List => cmx::list::list_kind(kind),
        ArtifactAction::Diff { name } => cmx::diff::diff(&name, kind).await,
        ArtifactAction::Update { name, all, force } => {
            if all {
                cmx::install::update_all(kind, force)
            } else if let Some(name) = name {
                cmx::install::update(&name, kind, force)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::Uninstall { name, local } => cmx::uninstall::uninstall(&name, kind, local),
    }
}
