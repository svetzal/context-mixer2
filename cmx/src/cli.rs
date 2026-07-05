use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::platform::Platform;

#[derive(Args, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputArgs {
    /// Emit machine-readable JSON instead of human-formatted output
    #[arg(long)]
    pub json: bool,
}

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
    /// Manage sets — named groups of installed artifacts with a desired
    /// activation state, activated/deactivated together
    Set {
        #[command(subcommand)]
        action: SetAction,
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
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Survey the whole system installation across every platform
    ///
    /// The survey itself is read-only: it mutates nothing and exists purely to
    /// make a disorganized installation visible. `--adopt-all` is a deprecated
    /// mutating shortcut kept for one release (see its own help).
    ///
    /// Exit codes:
    ///   0 - no issues found
    ///   2 - actionable issues found (drifted, untracked, orphaned, missing, or
    ///       diverged artifacts)
    #[command(verbatim_doc_comment)]
    Doctor {
        /// Also survey project (local) scope, not just global
        #[arg(long)]
        local: bool,
        /// Adopt every orphaned artifact into the canonical home (deprecated;
        /// use `cmx <kind> adopt --all`)
        #[arg(long = "adopt-all")]
        adopt_all: bool,
        /// With --adopt-all, only adopt orphans under this install directory
        /// (deprecated; use `--from` on `cmx <kind> adopt --all`)
        #[arg(long)]
        from: Option<PathBuf>,
        /// Show the full inventory, not just artifacts that need attention
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Manage the canonical home for hand-authored artifacts
    Home {
        #[command(subcommand)]
        action: HomeAction,
    },
    /// Show installed artifacts that have updates available
    Outdated {
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Search all sources for agents and skills by keyword
    Search {
        /// Keyword to search for in artifact names and descriptions
        query: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show detailed metadata for an installed artifact
    Info {
        /// Artifact name
        name: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// View or modify cmx configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Install cmx's own companion agent skill (global scope by default).
    ///
    /// Example: `cmx init` installs the `cmx` skill into `~/.claude/skills/`;
    /// `cmx init --local` installs into `.claude/skills/` in the current project.
    Init {
        /// Install into .claude/skills/ in the current project instead of ~/.claude/skills/
        #[arg(long)]
        local: bool,
        /// Deprecated: global is now the default. Accepted but ignored for one release.
        #[arg(long, hide = true)]
        global: bool,
        /// Overwrite even if the installed version is newer than the bundled version
        #[arg(long)]
        force: bool,
        /// Uninstall the cmx companion skill
        #[arg(long)]
        remove: bool,
        #[command(flatten)]
        output: OutputArgs,
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
pub enum SetAction {
    /// Create an empty, inactive set
    Create {
        /// Name to identify this set
        name: String,
        /// Human-readable description
        #[arg(long = "desc")]
        desc: Option<String>,
        /// Seed membership from a marketplace plugin's declared agents/skills
        #[arg(long, value_name = "source>:<plugin")]
        from: Option<String>,
        /// Create in project scope instead of global
        #[arg(long)]
        local: bool,
    },
    /// List defined sets
    List {
        /// List project-scoped sets instead of global
        #[arg(long)]
        local: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show a set's description, state, and members
    Show {
        /// Name of the set to show
        name: String,
        /// Look up the set in project scope instead of global
        #[arg(long)]
        local: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Add installed artifact(s) to a set, resolving kind and source from the
    /// lockfile. Use `skill:name` / `agent:name` to disambiguate a name that
    /// is ambiguous across kinds.
    Add {
        /// Name of the set to add to
        name: String,
        /// Artifact name(s), optionally prefixed with `skill:` or `agent:`
        artifacts: Vec<String>,
        /// Modify a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// Remove artifact(s) from a set (does NOT uninstall them)
    Remove {
        /// Name of the set to remove from
        name: String,
        /// Artifact name(s), optionally prefixed with `skill:` or `agent:`
        artifacts: Vec<String>,
        /// Modify a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// Install every member from its pinned source into the normally
    /// resolved install targets, and mark the set active. Idempotent — safe
    /// to re-run to repair a partially-installed set.
    Activate {
        /// Name of the set to activate
        name: String,
        /// Preview which members would install, without making any changes
        #[arg(long)]
        dry_run: bool,
        /// Act on a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// Uninstall every member not held by another active set, and mark the
    /// set inactive. A member with local edits blocks its own uninstall
    /// unless `--force` is passed.
    Deactivate {
        /// Name of the set to deactivate
        name: String,
        /// Preview what would be uninstalled/retained/blocked, without making any changes
        #[arg(long)]
        dry_run: bool,
        /// Discard local edits on drifted members instead of blocking on them
        #[arg(long)]
        force: bool,
        /// Act on a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// Delete a set's definition
    Delete {
        /// Name of the set to delete
        name: String,
        /// Modify a project-scoped set instead of global
        #[arg(long)]
        local: bool,
        /// Also deactivate (uninstall members not held by another active set) before deleting
        #[arg(long)]
        purge: bool,
        /// With --purge, discard local edits on drifted members instead of blocking on them
        #[arg(long)]
        force: bool,
    },
    /// Rename a set
    Rename {
        /// Current name
        old: String,
        /// New name
        new: String,
        /// Modify a project-scoped set instead of global
        #[arg(long)]
        local: bool,
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
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Show key details for an installed artifact: source, version, when it
    /// activates, and (in an `llm`-feature build) a summary of what it does
    Info {
        /// Artifact name
        name: String,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Compare an installed artifact against its source and other installed
    /// copies (an `llm`-feature build additionally summarizes the diff)
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
    /// installed, to make those edits the canonical copy. By default cmx picks
    /// the copy that was edited in place (the drifted one); if several platforms
    /// diverge, pass `--platform <name>` to choose which copy wins. Inspect the
    /// divergence first with `cmx <kind> diff <name>`.
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
    Path {
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show {
        #[command(flatten)]
        output: OutputArgs,
    },
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

    #[test]
    fn parse_set_create() {
        let cli =
            Cli::try_parse_from(["cmx", "set", "create", "rust-work", "--desc", "desc"]).unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Create {
                        name,
                        desc,
                        from,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert_eq!(desc.as_deref(), Some("desc"));
                assert!(from.is_none());
                assert!(!local);
            }
            _ => panic!("expected Set Create"),
        }
    }

    #[test]
    fn parse_set_create_from() {
        let cli = Cli::try_parse_from([
            "cmx",
            "set",
            "create",
            "rust-work",
            "--from",
            "guidelines:my-plugin",
        ])
        .unwrap();
        match cli.command {
            Commands::Set {
                action: SetAction::Create { name, from, .. },
            } => {
                assert_eq!(name, "rust-work");
                assert_eq!(from.as_deref(), Some("guidelines:my-plugin"));
            }
            _ => panic!("expected Set Create"),
        }
    }

    #[test]
    fn parse_set_list() {
        let cli = Cli::try_parse_from(["cmx", "set", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Set {
                action: SetAction::List {
                    local: false,
                    output: OutputArgs { json: false }
                }
            }
        ));
    }

    #[test]
    fn parse_set_show() {
        let cli = Cli::try_parse_from(["cmx", "set", "show", "rust-work"]).unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Show {
                        name,
                        local,
                        output,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(!local);
                assert!(!output.json);
            }
            _ => panic!("expected Set Show"),
        }
    }

    #[test]
    fn parse_set_add() {
        let cli =
            Cli::try_parse_from(["cmx", "set", "add", "rust-work", "skill:foundry", "agent-x"])
                .unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Add {
                        name,
                        artifacts,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert_eq!(artifacts, vec!["skill:foundry".to_string(), "agent-x".to_string()]);
                assert!(!local);
            }
            _ => panic!("expected Set Add"),
        }
    }

    #[test]
    fn parse_set_remove() {
        let cli = Cli::try_parse_from(["cmx", "set", "remove", "rust-work", "foundry", "--local"])
            .unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Remove {
                        name,
                        artifacts,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert_eq!(artifacts, vec!["foundry".to_string()]);
                assert!(local);
            }
            _ => panic!("expected Set Remove"),
        }
    }

    #[test]
    fn parse_set_activate() {
        let cli =
            Cli::try_parse_from(["cmx", "set", "activate", "rust-work", "--dry-run"]).unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Activate {
                        name,
                        dry_run,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(dry_run);
                assert!(!local);
            }
            _ => panic!("expected Set Activate"),
        }
    }

    #[test]
    fn parse_set_deactivate() {
        let cli =
            Cli::try_parse_from(["cmx", "set", "deactivate", "rust-work", "--force"]).unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Deactivate {
                        name,
                        dry_run,
                        force,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(!dry_run);
                assert!(force);
                assert!(!local);
            }
            _ => panic!("expected Set Deactivate"),
        }
    }

    #[test]
    fn parse_set_delete() {
        let cli = Cli::try_parse_from(["cmx", "set", "delete", "rust-work", "--purge", "--force"])
            .unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Delete {
                        name,
                        local,
                        purge,
                        force,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(!local);
                assert!(purge);
                assert!(force);
            }
            _ => panic!("expected Set Delete"),
        }
    }

    #[test]
    fn parse_set_rename() {
        let cli = Cli::try_parse_from(["cmx", "set", "rename", "old", "new"]).unwrap();
        match cli.command {
            Commands::Set {
                action: SetAction::Rename { old, new, local },
            } => {
                assert_eq!(old, "old");
                assert_eq!(new, "new");
                assert!(!local);
            }
            _ => panic!("expected Set Rename"),
        }
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
                action: ArtifactAction::Info { name, output },
            } => {
                assert_eq!(name, "my-skill");
                assert!(!output.json);
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

    #[test]
    fn parse_list() {
        let cli = Cli::try_parse_from(["cmx", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::List { .. }));
    }

    #[test]
    fn parse_outdated() {
        let cli = Cli::try_parse_from(["cmx", "outdated"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Outdated {
                output: OutputArgs { json: false }
            }
        ));
    }

    #[test]
    fn parse_search() {
        let cli = Cli::try_parse_from(["cmx", "search", "foo"]).unwrap();
        match cli.command {
            Commands::Search { query, output } => {
                assert_eq!(query, "foo");
                assert!(!output.json);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_info() {
        let cli = Cli::try_parse_from(["cmx", "info", "myagent"]).unwrap();
        match cli.command {
            Commands::Info { name, output } => {
                assert_eq!(name, "myagent");
                assert!(!output.json);
            }
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
                action: HomeAction::Path {
                    output: OutputArgs { json: false }
                }
            }
        ));
    }

    #[test]
    fn parse_doctor() {
        let cli = Cli::try_parse_from(["cmx", "doctor"]).unwrap();
        assert!(matches!(cli.command, Commands::Doctor { .. }));
    }

    #[test]
    fn parse_init_defaults() {
        let cli = Cli::try_parse_from(["cmx", "init"]).unwrap();
        match cli.command {
            Commands::Init {
                local,
                global,
                force,
                remove,
                output,
            } => {
                assert!(!local);
                assert!(!global);
                assert!(!force);
                assert!(!remove);
                assert!(!output.json);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_init_all_flags() {
        let cli = Cli::try_parse_from([
            "cmx", "init", "--local", "--global", "--force", "--remove", "--json",
        ])
        .unwrap();
        match cli.command {
            Commands::Init {
                local,
                global,
                force,
                remove,
                output,
            } => {
                assert!(local);
                assert!(global);
                assert!(force);
                assert!(remove);
                assert!(output.json);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_init_distinct_from_home_init() {
        let init = Cli::try_parse_from(["cmx", "init"]).unwrap();
        let home_init = Cli::try_parse_from(["cmx", "home", "init"]).unwrap();
        assert!(matches!(init.command, Commands::Init { .. }));
        assert!(matches!(
            home_init.command,
            Commands::Home {
                action: HomeAction::Init
            }
        ));
    }

    #[test]
    fn parse_invalid_command_errors() {
        assert!(Cli::try_parse_from(["cmx", "notacommand"]).is_err());
    }
}
