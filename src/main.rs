mod cli;
mod config;
mod install;
mod list;
mod scan;
mod source;
mod types;

use anyhow::Result;
use clap::Parser;

use cli::{ArtifactAction, Cli, Commands, SourceAction};
use types::ArtifactKind;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Source { action } => match action {
            SourceAction::Add { name, path_or_url } => source::add(&name, &path_or_url),
            SourceAction::List => source::list(),
            SourceAction::Browse { name } => source::browse(&name),
            SourceAction::Pull { name } => source::pull(&name),
            SourceAction::Remove { name } => source::remove(&name),
        },
        Commands::Agent { action } => match action {
            ArtifactAction::Install { name, local } => {
                install::install(&name, ArtifactKind::Agent, local)
            }
            ArtifactAction::List => list::list_kind(ArtifactKind::Agent),
        },
        Commands::Skill { action } => match action {
            ArtifactAction::Install { name, local } => {
                install::install(&name, ArtifactKind::Skill, local)
            }
            ArtifactAction::List => list::list_kind(ArtifactKind::Skill),
        },
        Commands::List => list::list_all(),
    }
}
