use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::platform::Platform;

#[derive(Parser)]
#[command(
    name = "cmx",
    about = "Package manager for curated agentic context — agents and skills",
    version
)]
pub struct Cli {
    /// Target AI coding assistant platform (env: `CMX_PLATFORM`).
    ///
    /// When omitted, `install`/`uninstall` act across every platform already in
    /// use (those with tracked artifacts); other commands default to Claude.
    /// Pass this to constrain an operation to a single platform.
    #[arg(long, value_enum, global = true, env = "CMX_PLATFORM")]
    pub platform: Option<Platform>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage source repositories
    Source {
        #[command(subcommand)]
        action: SourceAction,
    },
    /// Manage agents
    Agent {
        #[command(subcommand)]
        action: ArtifactAction,
    },
    /// Manage skills
    Skill {
        #[command(subcommand)]
        action: ArtifactAction,
    },
    /// List all installed agents and skills
    List {
        /// Include external artifacts (managed by another tool)
        #[arg(long)]
        all: bool,
    },
    /// Survey the whole system installation across every platform (read-only)
    Doctor {
        /// Also survey project (local) scope, not just global
        #[arg(long)]
        local: bool,
        /// Adopt every orphaned artifact into the canonical home (mutating)
        #[arg(long = "adopt-all")]
        adopt_all: bool,
        /// With --adopt-all, only adopt orphans under this install directory
        #[arg(long)]
        from: Option<PathBuf>,
        /// Show the full inventory, not just artifacts that need attention
        #[arg(long)]
        all: bool,
    },
    /// Manage the canonical home for hand-authored artifacts
    Home {
        #[command(subcommand)]
        action: HomeAction,
    },
    /// Show installed artifacts that have updates available
    Outdated,
    /// Search all sources for agents and skills by keyword
    Search {
        /// Keyword to search for in artifact names and descriptions
        query: String,
    },
    /// Show detailed metadata for an installed artifact
    Info {
        /// Artifact name
        name: String,
    },
    /// View or modify cmx configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum SourceAction {
    /// Register a source repository (local path or git URL)
    Add {
        /// Name to identify this source
        name: String,
        /// Local path or URL to the source repository
        path_or_url: String,
    },
    /// List registered sources
    List,
    /// Show available agents and skills in a source
    Browse {
        /// Name of the source to browse
        name: String,
    },
    /// Fetch latest changes for git-backed sources
    Update {
        /// Name of a specific source to update (default: all)
        name: Option<String>,
    },
    /// Unregister a source (does not delete artifacts)
    Remove {
        /// Name of the source to remove
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ArtifactAction {
    /// Install artifact(s) from a source
    Install {
        /// Artifact name(s); each may be `source:name` to pin to a specific source
        names: Vec<String>,
        /// Install all available artifacts from sources
        #[arg(long, conflicts_with = "names")]
        all: bool,
        /// Install into the current project instead of globally
        #[arg(long)]
        local: bool,
        /// Force overwrite even if locally modified
        #[arg(long)]
        force: bool,
    },
    /// List installed artifacts
    List {
        /// Include external artifacts (managed by another tool)
        #[arg(long)]
        all: bool,
    },
    /// Show key details for an installed artifact: source, version, when it
    /// activates, and (in an `llm`-feature build) a summary of what it does
    Info {
        /// Artifact name
        name: String,
    },
    #[cfg(feature = "llm")]
    /// Compare installed artifact with source version using LLM analysis
    Diff {
        /// Artifact name
        name: String,
        /// Show the full line-by-line unified diff (default: compact summary)
        #[arg(long)]
        full: bool,
    },
    /// Update an installed artifact from its source
    Update {
        /// Artifact name
        name: Option<String>,
        /// Update all tracked artifacts
        #[arg(long, conflicts_with = "name")]
        all: bool,
        /// Force overwrite even if locally modified
        #[arg(long)]
        force: bool,
    },
    /// Reconcile a skill that has diverged across platforms by copying one
    /// copy over the others. Unlike `update` (which pulls from a source), `sync`
    /// works between install locations — so it also reconciles `external` skills.
    Sync {
        /// Skill name to reconcile
        name: String,
        /// Platform whose copy wins (default: the newest version)
        #[arg(long, value_enum)]
        from: Option<Platform>,
        /// Preview the reconciliation without writing anything
        #[arg(long)]
        dry_run: bool,
        /// Reconcile within project scope instead of global
        #[arg(long)]
        local: bool,
    },
    /// Promote in-place edits of an installed artifact back into the canonical
    /// home — the mirror of `update`. Use after editing a skill where it's
    /// installed, to make those edits the canonical copy. Promotes the copy
    /// `cmx diff` shows (global scope preferred, then project).
    Promote {
        /// Artifact name to promote into the home
        name: String,
    },
    /// Uninstall installed artifact(s) — removed everywhere cmx tracks them
    Uninstall {
        /// Artifact name(s) to uninstall
        names: Vec<String>,
        /// Uninstall from project scope instead of global
        #[arg(long)]
        local: bool,
    },
    /// Unadopt artifact(s): remove them from the canonical home and un-track them
    Unadopt {
        /// Artifact name(s) to unadopt
        names: Vec<String>,
        /// Also mark each as external (managed by another tool) after unadopting
        #[arg(long)]
        external: bool,
    },
    /// Adopt orphaned, hand-authored artifacts into the canonical home
    Adopt {
        /// Artifact name(s) to adopt (each must be an orphan reported by `cmx doctor`)
        names: Vec<String>,
        /// Adopt all orphans of this kind instead of named ones
        #[arg(long, conflicts_with = "names")]
        all: bool,
        /// With --all, only adopt orphans under this install directory
        #[arg(long)]
        from: Option<PathBuf>,
        /// Search project (local) scope as well as global for orphans
        #[arg(long)]
        local: bool,
    },
}

#[derive(Subcommand)]
pub enum HomeAction {
    /// Create the canonical home directory and register it as the `home` source
    Init,
    /// Print the resolved canonical home directory
    Path,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set LLM gateway (openai or ollama)
    Gateway {
        /// Gateway type: openai or ollama
        value: String,
    },
    /// Set LLM model name
    Model {
        /// Model name (e.g. gpt-5.4, qwen3.5:27b)
        value: String,
    },
    /// Manage `external` rules — artifacts another tool manages, which `doctor`
    /// reports as external instead of flagging
    External {
        #[command(subcommand)]
        action: ExternalAction,
    },
    /// Manage the set of platforms cmx manages. When set, `install`/`uninstall`
    /// act on exactly these and `doctor` surveys only these; when empty, cmx
    /// infers the set from the platforms already in use.
    Platforms {
        #[command(subcommand)]
        action: PlatformsAction,
    },
}

#[derive(Subcommand)]
pub enum PlatformsAction {
    /// List the platforms cmx manages
    List,
    /// Add a platform to the managed set
    Add {
        /// Platform to manage (e.g. claude, codex)
        #[arg(value_enum)]
        platform: Platform,
    },
    /// Remove a platform from the managed set
    Remove {
        /// Platform to stop managing
        #[arg(value_enum)]
        platform: Platform,
    },
}

#[derive(Subcommand)]
pub enum ExternalAction {
    /// List the configured external rules
    List,
    /// Add an external rule: a directory (e.g. ~/.hermes/skills) or an artifact name
    Add {
        /// Directory path or bare artifact name to mark external
        entry: String,
    },
    /// Remove an external rule
    Remove {
        /// The directory path or name to remove
        entry: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

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
        let cli = Cli::try_parse_from(["cmx", "source", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Source {
                action: SourceAction::List
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

    #[test]
    fn parse_agent_install() {
        let cli = Cli::try_parse_from(["cmx", "agent", "install", "foo"]).unwrap();
        match cli.command {
            Commands::Agent {
                action: ArtifactAction::Install { names, .. },
            } => {
                assert_eq!(names, vec!["foo"]);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_agent_install_all() {
        let cli = Cli::try_parse_from(["cmx", "agent", "install", "--all"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Agent {
                action: ArtifactAction::Install { all: true, .. }
            }
        ));
    }

    #[test]
    fn parse_agent_list() {
        let cli = Cli::try_parse_from(["cmx", "agent", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Agent {
                action: ArtifactAction::List { .. }
            }
        ));
    }

    #[test]
    fn parse_skill_info() {
        let cli = Cli::try_parse_from(["cmx", "skill", "info", "my-skill"]).unwrap();
        match cli.command {
            Commands::Skill {
                action: ArtifactAction::Info { name },
            } => {
                assert_eq!(name, "my-skill");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_config_show() {
        let cli = Cli::try_parse_from(["cmx", "config", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Config {
                action: ConfigAction::Show
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

    #[test]
    fn parse_list() {
        let cli = Cli::try_parse_from(["cmx", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::List { .. }));
    }

    #[test]
    fn parse_outdated() {
        let cli = Cli::try_parse_from(["cmx", "outdated"]).unwrap();
        assert!(matches!(cli.command, Commands::Outdated));
    }

    #[test]
    fn parse_search() {
        let cli = Cli::try_parse_from(["cmx", "search", "foo"]).unwrap();
        match cli.command {
            Commands::Search { query } => assert_eq!(query, "foo"),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_info() {
        let cli = Cli::try_parse_from(["cmx", "info", "myagent"]).unwrap();
        match cli.command {
            Commands::Info { name } => assert_eq!(name, "myagent"),
            _ => panic!("unexpected command"),
        }
    }

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
                action: HomeAction::Path
            }
        ));
    }

    #[test]
    fn parse_doctor() {
        let cli = Cli::try_parse_from(["cmx", "doctor"]).unwrap();
        assert!(matches!(cli.command, Commands::Doctor { .. }));
    }

    #[test]
    fn parse_invalid_command_errors() {
        assert!(Cli::try_parse_from(["cmx", "notacommand"]).is_err());
    }
}
