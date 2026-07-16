use clap::Subcommand;

use super::OutputArgs;

#[derive(Subcommand)]
pub enum SourceAction {
    /// [Mutates] Register a source repository (local path or git URL)
    Add {
        /// Name to identify this source
        name: String,
        /// Local path or URL to the source repository
        path_or_url: String,
    },
    /// List registered sources
    List {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show available agents and skills in a source
    Browse {
        /// Name of the source to browse
        name: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// [Mutates] Fetch latest changes for git-backed sources
    Update {
        /// Name of a specific source to update (default: all)
        name: Option<String>,
    },
    /// [Mutates] Unregister a source (does not delete artifacts)
    Remove {
        /// Name of the source to remove
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::super::{Cli, Commands};
    use super::SourceAction;

    #[test]
    fn parse_source_add() {
        let cli = Cli::try_parse_from(["cmx", "source", "add", "myrepo", "/path"]).unwrap();
        match cli.command {
            Commands::Source {
                action: SourceAction::Add { name, path_or_url },
            } => {
                assert_eq!(name, "myrepo");
                assert_eq!(path_or_url, "/path");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_source_list() {
        use super::super::OutputArgs;
        let cli = Cli::try_parse_from(["cmx", "source", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Source {
                action: SourceAction::List {
                    output: OutputArgs { json: false }
                }
            }
        ));
    }

    #[test]
    fn parse_source_update_all() {
        let cli = Cli::try_parse_from(["cmx", "source", "update"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Source {
                action: SourceAction::Update { name: None }
            }
        ));
    }

    #[test]
    fn parse_source_update_named() {
        let cli = Cli::try_parse_from(["cmx", "source", "update", "myrepo"]).unwrap();
        match cli.command {
            Commands::Source {
                action: SourceAction::Update { name },
            } => {
                assert_eq!(name, Some("myrepo".to_string()));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_source_remove() {
        let cli = Cli::try_parse_from(["cmx", "source", "remove", "myrepo"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Source {
                action: SourceAction::Remove { .. }
            }
        ));
    }
}
