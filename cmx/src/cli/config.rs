//! `cmx config` subcommand argument definitions, part of the clap CLI
//! defined in `cmx/src/cli/mod.rs`.

use clap::Subcommand;

use crate::platform::Platform;

use super::OutputArgs;

/// `cmx config` subcommands.
#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show {
        #[command(flatten)]
        /// Shared `--json` output flag.
        output: OutputArgs,
    },
    /// [Mutates] Set LLM gateway (openai or ollama)
    Gateway {
        /// Gateway type: openai or ollama
        value: String,
    },
    /// [Mutates] Set LLM model name
    Model {
        /// Model name (e.g. gpt-5.4, qwen3.5:27b)
        value: String,
    },
    /// Manage `external` rules — artifacts another tool manages, which `doctor`
    /// reports as external instead of flagging
    External {
        #[command(subcommand)]
        /// The external-rule operation to perform.
        action: ExternalAction,
    },
    /// Manage the set of platforms cmx manages. When set, `install`/`uninstall`
    /// act on exactly these and `doctor` surveys only these; when empty, cmx
    /// infers the set from the platforms already in use.
    Platforms {
        #[command(subcommand)]
        /// The managed-platform-set operation to perform.
        action: PlatformsAction,
    },
}

/// `cmx config platforms` subcommands.
#[derive(Subcommand)]
pub enum PlatformsAction {
    /// List the platforms cmx manages
    List,
    /// [Mutates] Add a platform to the managed set
    Add {
        /// Platform to manage (e.g. claude, codex)
        #[arg(value_enum)]
        platform: Platform,
    },
    /// [Mutates] Remove a platform from the managed set
    Remove {
        /// Platform to stop managing
        #[arg(value_enum)]
        platform: Platform,
    },
}

/// `cmx config external` subcommands.
#[derive(Subcommand)]
pub enum ExternalAction {
    /// List the configured external rules
    List,
    /// [Mutates] Add an external rule: a directory (e.g. ~/.hermes/skills) or an artifact name
    Add {
        /// Directory path or bare artifact name to mark external
        entry: String,
    },
    /// [Mutates] Remove an external rule
    Remove {
        /// The directory path or name to remove
        entry: String,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::super::{Cli, Commands, OutputArgs};
    use super::{ConfigAction, ExternalAction};

    #[test]
    fn parse_config_show() {
        let cli = Cli::try_parse_from(["cmx", "config", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Config {
                action: ConfigAction::Show {
                    output: OutputArgs { json: false }
                }
            }
        ));
    }

    #[test]
    fn parse_config_gateway() {
        let cli = Cli::try_parse_from(["cmx", "config", "gateway", "openai"]).unwrap();
        match cli.command {
            Commands::Config {
                action: ConfigAction::Gateway { value },
            } => {
                assert_eq!(value, "openai");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_config_model() {
        let cli = Cli::try_parse_from(["cmx", "config", "model", "gpt-4"]).unwrap();
        match cli.command {
            Commands::Config {
                action: ConfigAction::Model { value },
            } => {
                assert_eq!(value, "gpt-4");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_config_external_list() {
        let cli = Cli::try_parse_from(["cmx", "config", "external", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Config {
                action: ConfigAction::External {
                    action: ExternalAction::List
                }
            }
        ));
    }
}
