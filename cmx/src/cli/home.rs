//! Canonical-home-related subcommand argument definitions, part of the clap
//! CLI defined in `cmx/src/cli/mod.rs`.

use clap::Subcommand;

use super::OutputArgs;

/// `cmx home` subcommands.
#[derive(Subcommand)]
pub enum HomeAction {
    /// [Mutates] Create the canonical home directory and register it as the `home` source
    Init,
    /// Print the resolved canonical home directory
    Path {
        #[command(flatten)]
        /// Shared `--json` output flag.
        output: OutputArgs,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::super::{Cli, Commands, OutputArgs};
    use super::HomeAction;

    #[test]
    fn parse_home_init() {
        let cli = Cli::try_parse_from(["cmx", "home", "init"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Home {
                action: HomeAction::Init
            }
        ));
    }

    #[test]
    fn parse_home_path() {
        let cli = Cli::try_parse_from(["cmx", "home", "path"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Home {
                action: HomeAction::Path {
                    output: OutputArgs { json: false }
                }
            }
        ));
    }
}
