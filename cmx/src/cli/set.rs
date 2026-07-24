//! `cmx set` subcommand argument definitions, part of the clap CLI defined
//! in `cmx/src/cli/mod.rs`.

use clap::Subcommand;

use super::OutputArgs;

/// `cmx set` subcommands.
#[derive(Subcommand)]
pub enum SetAction {
    /// [Mutates] Create an empty, inactive set
    Create {
        /// Name to identify this set
        name: String,
        /// Human-readable description
        #[arg(long = "desc")]
        desc: Option<String>,
        /// Seed membership from a marketplace plugin's declared agents/skills
        #[arg(
            long = "from-plugin",
            value_name = "source>:<plugin",
            conflicts_with = "deprecated_from"
        )]
        from_plugin: Option<String>,
        /// Deprecated: use --from-plugin
        #[arg(long = "from", hide = true, conflicts_with = "from_plugin")]
        deprecated_from: Option<String>,
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
        /// Shared `--json` output flag.
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
        /// Shared `--json` output flag.
        output: OutputArgs,
    },
    /// [Mutates] Add installed artifact(s) to a set, resolving kind and source from the
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
    /// [Mutates] Remove artifact(s) from a set (does NOT uninstall them)
    Remove {
        /// Name of the set to remove from
        name: String,
        /// Artifact name(s), optionally prefixed with `skill:` or `agent:`
        artifacts: Vec<String>,
        /// Modify a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// [Mutates with --apply] Install every member from its pinned source into the normally
    /// resolved install targets, and mark the set active. Idempotent — safe
    /// to re-run to repair a partially-installed set.
    Activate {
        /// Name of the set to activate
        name: String,
        /// Execute the activation after showing the concrete plan
        #[arg(long)]
        apply: bool,
        /// Deprecated: the activation plan is now shown by default; pass --apply to execute
        #[arg(long, hide = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Act on a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// [Mutates with --apply] Uninstall every member not held by another active set, and mark the
    /// set inactive. A member with local edits blocks its own uninstall
    /// unless `--force` is passed.
    Deactivate {
        /// Name of the set to deactivate
        name: String,
        /// Execute the deactivation after showing the concrete plan
        #[arg(long)]
        apply: bool,
        /// Deprecated: the deactivation plan is now shown by default; pass --apply to execute
        #[arg(long, hide = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Discard local edits on drifted members instead of blocking on them
        #[arg(long)]
        force: bool,
        /// Act on a project-scoped set instead of global
        #[arg(long)]
        local: bool,
    },
    /// [Mutates] Delete a set's definition
    Delete {
        /// Name of the set to delete
        name: String,
        /// Modify a project-scoped set instead of global
        #[arg(long)]
        local: bool,
        /// Also deactivate first; previewed by default and only executed with --apply
        #[arg(long)]
        purge: bool,
        /// With --purge, execute the purge after showing the concrete plan
        #[arg(long)]
        apply: bool,
        /// With --purge, discard local edits on drifted members instead of blocking on them
        #[arg(long)]
        force: bool,
    },
    /// [Mutates] Rename a set
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, error::ErrorKind};

    use super::super::{Cli, Commands, OutputArgs};
    use super::SetAction;

    fn rendered_help(args: &[&str]) -> String {
        let err = Cli::command()
            .try_get_matches_from_mut(args)
            .expect_err("help flag should short-circuit parsing");
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        err.to_string()
    }

    fn help_for(path: &[&str]) -> String {
        let mut args = Vec::with_capacity(path.len() + 2);
        args.push("cmx");
        args.extend_from_slice(path);
        args.push("--help");
        rendered_help(&args)
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
                        from_plugin,
                        deprecated_from,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert_eq!(desc.as_deref(), Some("desc"));
                assert!(from_plugin.is_none());
                assert!(deprecated_from.is_none());
                assert!(!local);
            }
            _ => panic!("expected Set Create"),
        }
    }

    #[test]
    fn parse_set_create_from_plugin() {
        let cli = Cli::try_parse_from([
            "cmx",
            "set",
            "create",
            "rust-work",
            "--from-plugin",
            "guidelines:my-plugin",
        ])
        .unwrap();
        match cli.command {
            Commands::Set {
                action:
                    SetAction::Create {
                        name,
                        from_plugin,
                        deprecated_from,
                        ..
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert_eq!(from_plugin.as_deref(), Some("guidelines:my-plugin"));
                assert!(deprecated_from.is_none());
            }
            _ => panic!("expected Set Create"),
        }
    }

    #[test]
    fn parse_set_create_deprecated_from_alias() {
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
                action:
                    SetAction::Create {
                        from_plugin,
                        deprecated_from,
                        ..
                    },
            } => {
                assert!(from_plugin.is_none());
                assert_eq!(deprecated_from.as_deref(), Some("guidelines:my-plugin"));
            }
            _ => panic!("expected Set Create"),
        }
    }

    #[test]
    fn parse_set_create_rejects_new_and_deprecated_flags_together() {
        let err = Cli::try_parse_from([
            "cmx",
            "set",
            "create",
            "rust-work",
            "--from-plugin",
            "guidelines:new-plugin",
            "--from",
            "guidelines:old-plugin",
        ])
        .err()
        .expect("new and deprecated flags should conflict")
        .to_string();
        assert!(err.contains("cannot be used with"), "{err}");
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
                        apply,
                        dry_run,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(!apply);
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
                        apply,
                        dry_run,
                        force,
                        local,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(!apply);
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
                        apply,
                        force,
                    },
            } => {
                assert_eq!(name, "rust-work");
                assert!(!local);
                assert!(purge);
                assert!(!apply);
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
    fn mutating_help_marks_plan_apply_commands_and_hides_dry_run() {
        let help = help_for(&["set", "activate"]);
        assert!(help.contains("[Mutates with --apply]"), "{help}");
        assert!(help.contains("--apply"), "{help}");
        assert!(!help.contains("--dry-run"), "{help}");
    }

    #[test]
    fn read_only_help_does_not_gain_mutation_flags() {
        let help = help_for(&["set", "list"]);
        assert!(!help.contains("[Mutates"), "{help}");
        assert!(!help.contains("--apply"), "{help}");
        assert!(!help.contains("--dry-run"), "{help}");
    }
}
