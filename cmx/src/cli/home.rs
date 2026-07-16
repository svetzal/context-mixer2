use clap::Subcommand;

use super::OutputArgs;

#[derive(Subcommand)]
pub enum HomeAction {
    /// [Mutates] Create the canonical home directory and register it as the `home` source
    Init,
    /// Print the resolved canonical home directory
    Path {
        #[command(flatten)]
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
