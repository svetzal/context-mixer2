//! `cmx agent` / `cmx skill` artifact subcommand argument definitions, part
//! of the clap CLI defined in `cmx/src/cli/mod.rs`.

use std::path::PathBuf;

use clap::Subcommand;

use crate::platform::Platform;

use super::OutputArgs;

/// `cmx agent` / `cmx skill` subcommands.
#[derive(Subcommand)]
pub enum ArtifactAction {
    /// [Mutates] Install artifact(s) from a source
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
        /// Shared `--json` output flag.
        output: OutputArgs,
    },
    /// Show key details for an installed artifact: source, version, when it
    /// activates, and (in an `llm`-feature build) a summary of what it does
    Info {
        /// Artifact name
        name: String,
        #[command(flatten)]
        /// Shared `--json` output flag.
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
    /// [Mutates] Update an installed artifact from its source
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
    /// [Mutates with --apply] Reconcile a skill that has diverged across platforms by copying one
    /// copy over the others. Unlike `update` (which pulls from a source), `sync`
    /// works between install locations — so it also reconciles `external` skills.
    Sync {
        /// Skill name to reconcile
        name: String,
        /// Platform whose copy wins (default: the newest version)
        #[arg(long, value_enum)]
        from: Option<Platform>,
        /// Execute the reconciliation after showing the concrete plan
        #[arg(long)]
        apply: bool,
        /// Deprecated: the reconciliation plan is now shown by default; pass --apply to execute
        #[arg(long, hide = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Reconcile within project scope instead of global
        #[arg(long)]
        local: bool,
    },
    /// [Mutates with --apply] Promote in-place edits of an installed artifact back into the canonical
    /// home — the mirror of `update`. Use after editing a skill where it's
    /// installed, to make those edits the canonical copy. By default cmx picks
    /// the copy that was edited in place (the drifted one); if several platforms
    /// diverge, pass `--from <name>` to choose which copy wins. Inspect the
    /// divergence first with `cmx <kind> diff <name>`.
    Promote {
        /// Artifact name to promote into the home
        name: String,
        /// Platform whose copy wins (default: the drifted copy)
        #[arg(long, value_enum)]
        from: Option<Platform>,
        /// Execute the promotion after showing the concrete plan
        #[arg(long)]
        apply: bool,
    },
    /// [Mutates] Uninstall installed artifact(s) — removed everywhere cmx tracks them
    Uninstall {
        /// Artifact name(s) to uninstall
        names: Vec<String>,
        /// Uninstall from project scope instead of global
        #[arg(long)]
        local: bool,
    },
    /// [Mutates] Unadopt artifact(s): remove them from the canonical home and un-track them
    Unadopt {
        /// Artifact name(s) to unadopt
        names: Vec<String>,
        /// Also mark each as external (managed by another tool) after unadopting
        #[arg(long)]
        external: bool,
    },
    /// [Mutates] Adopt orphaned, hand-authored artifacts into the canonical home
    Adopt {
        /// Artifact name(s) to adopt (each must be an orphan reported by `cmx doctor`)
        names: Vec<String>,
        /// Adopt all orphans of this kind instead of named ones
        #[arg(long, conflicts_with = "names")]
        all: bool,
        /// With --all, only adopt orphans under this install directory
        #[arg(long = "from-dir", conflicts_with = "deprecated_from")]
        from_dir: Option<PathBuf>,
        /// Deprecated: use --from-dir
        #[arg(long = "from", hide = true, conflicts_with = "from_dir")]
        deprecated_from: Option<PathBuf>,
        /// Search project (local) scope as well as global for orphans
        #[arg(long)]
        local: bool,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::super::{Cli, Commands};
    use super::ArtifactAction;
    use crate::platform::Platform;

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
    fn parse_skill_promote_from() {
        let cli = Cli::try_parse_from(["cmx", "skill", "promote", "my-skill", "--from", "codex"])
            .unwrap();
        match cli.command {
            Commands::Skill {
                action: ArtifactAction::Promote { name, from, apply },
            } => {
                assert_eq!(name, "my-skill");
                assert_eq!(from, Some(Platform::Codex));
                assert!(!apply);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_skill_adopt_from_dir() {
        let cli = Cli::try_parse_from([
            "cmx",
            "skill",
            "adopt",
            "--all",
            "--from-dir",
            "/tmp/skills",
        ])
        .unwrap();
        match cli.command {
            Commands::Skill {
                action:
                    ArtifactAction::Adopt {
                        names,
                        all,
                        from_dir,
                        deprecated_from,
                        local,
                    },
            } => {
                assert!(names.is_empty());
                assert!(all);
                assert_eq!(from_dir, Some(PathBuf::from("/tmp/skills")));
                assert!(deprecated_from.is_none());
                assert!(!local);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_skill_adopt_deprecated_from_alias() {
        let cli = Cli::try_parse_from(["cmx", "skill", "adopt", "--all", "--from", "/tmp/skills"])
            .unwrap();
        match cli.command {
            Commands::Skill {
                action:
                    ArtifactAction::Adopt {
                        from_dir,
                        deprecated_from,
                        ..
                    },
            } => {
                assert!(from_dir.is_none());
                assert_eq!(deprecated_from, Some(PathBuf::from("/tmp/skills")));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parse_skill_adopt_rejects_new_and_deprecated_flags_together() {
        let err = Cli::try_parse_from([
            "cmx",
            "skill",
            "adopt",
            "--all",
            "--from-dir",
            "/tmp/new",
            "--from",
            "/tmp/old",
        ])
        .err()
        .expect("new and deprecated flags should conflict")
        .to_string();
        assert!(err.contains("cannot be used with"), "{err}");
    }
}
