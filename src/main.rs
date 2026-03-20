mod checksum;
mod cli;
mod cmx_config;
mod config;
mod diff;
mod install;
mod list;
mod lockfile;
mod outdated;
mod scan;
mod source;
mod types;

use anyhow::{Result, bail};
use clap::Parser;

use cli::{ArtifactAction, Cli, Commands, ConfigAction, SourceAction};
use types::ArtifactKind;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Source { action } => match action {
            SourceAction::Add { name, path_or_url } => source::add(&name, &path_or_url),
            SourceAction::List => source::list(),
            SourceAction::Browse { name } => source::browse(&name),
            SourceAction::Update { name } => source::update(name.as_deref()),
            SourceAction::Remove { name } => source::remove(&name),
        },
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent).await,
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill).await,
        Commands::List => list::list_all(),
        Commands::Outdated => outdated::outdated(),
        Commands::Config { action } => match action {
            ConfigAction::Show => cmx_config::show(),
            ConfigAction::Gateway { value } => cmx_config::set_gateway(&value),
            ConfigAction::Model { value } => cmx_config::set_model(&value),
        },
    }
}

async fn handle_artifact(action: ArtifactAction, kind: ArtifactKind) -> Result<()> {
    match action {
        ArtifactAction::Install { name, all, local } => {
            if all {
                install::install_all(kind, local)
            } else if let Some(name) = name {
                install::install(&name, kind, local)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::List => list::list_kind(kind),
        ArtifactAction::Diff { name } => diff::diff(&name, kind).await,
        ArtifactAction::Update { name, all } => {
            if all {
                install::update_all(kind)
            } else if let Some(name) = name {
                install::update(&name, kind)
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
    }
}
