//! clap CLI definition: imports, `COMPLETIONS_LONG_HELP`, `OutputArgs`, `Cli`,
//! `Commands`; re-exports all action enums from its submodules
//! (`cmx/src/cli/source.rs`, `cmx/src/cli/set.rs`, `cmx/src/cli/artifact.rs`,
//! `cmx/src/cli/home.rs`, `cmx/src/cli/config.rs`).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

use crate::platform::{PLATFORM_HELP_VALUES, Platform};

mod artifact;
mod config;
mod home;
mod set;
mod source;

pub use artifact::ArtifactAction;
pub use config::{ConfigAction, ExternalAction, PlatformsAction};
pub use home::HomeAction;
pub use set::SetAction;
pub use source::SourceAction;

const COMPLETIONS_LONG_HELP: &str = "\
Generate a shell completion script to stdout.

Supported shells: bash, zsh, fish, elvish, powershell

Examples:
  cmx completions zsh > ~/.zfunc/_cmx
    Then add `~/.zfunc` to `fpath` and run `autoload -Uz compinit && compinit`.

  cmx completions bash | sudo tee /etc/bash_completion.d/cmx >/dev/null
";

/// Shared `--json` flag flattened into commands that support machine-readable output.
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
    after_long_help = PLATFORM_HELP_VALUES,
    version
)]
/// Top-level parsed command line: the target platform override and the chosen subcommand.
pub struct Cli {
    #[arg(
        long,
        value_enum,
        hide_possible_values = true,
        global = true,
        env = "CMX_PLATFORM",
        help = "Target AI coding assistant platform (see 'cmx --help' for the full list)",
        long_help = "Target AI coding assistant platform (see 'cmx --help' for the full list)"
    )]
    /// Global target AI coding assistant platform override.
    pub platform: Option<Platform>,

    #[command(subcommand)]
    /// The subcommand to execute.
    pub command: Commands,
}

/// All top-level cmx subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Manage source repositories
    Source {
        #[command(subcommand)]
        /// The source subcommand to run.
        action: SourceAction,
    },
    /// Manage sets — named groups of installed artifacts with a desired
    /// activation state, activated/deactivated together
    Set {
        #[command(subcommand)]
        /// The set subcommand to run.
        action: SetAction,
    },
    /// Manage agents
    Agent {
        #[command(subcommand)]
        /// The agent subcommand to run.
        action: ArtifactAction,
    },
    /// Manage skills
    Skill {
        #[command(subcommand)]
        /// The skill subcommand to run.
        action: ArtifactAction,
    },
    /// List all installed agents and skills
    List {
        /// Include external artifacts (managed by another tool)
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        /// Output formatting flags.
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
        /// (deprecated; use `--from-dir` on `cmx <kind> adopt --all`)
        #[arg(long)]
        from: Option<PathBuf>,
        /// Show the full inventory, not just artifacts that need attention
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        /// Output formatting flags.
        output: OutputArgs,
    },
    /// Manage the canonical home for hand-authored artifacts
    Home {
        #[command(subcommand)]
        /// The home subcommand to run.
        action: HomeAction,
    },
    /// Show installed artifacts that have updates available
    Outdated {
        #[command(flatten)]
        /// Output formatting flags.
        output: OutputArgs,
    },
    /// Search all sources for agents and skills by keyword
    Search {
        /// Keyword to search for in artifact names and descriptions
        query: String,
        #[command(flatten)]
        /// Output formatting flags.
        output: OutputArgs,
    },
    /// Show detailed metadata for an installed artifact
    Info {
        /// Artifact name
        name: String,
        #[command(flatten)]
        /// Output formatting flags.
        output: OutputArgs,
    },
    #[command(
        about = "Generate shell completion script",
        long_about = COMPLETIONS_LONG_HELP
    )]
    /// Emit a shell completion script for the requested shell.
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// View or modify cmx configuration
    Config {
        #[command(subcommand)]
        /// The config subcommand to run.
        action: ConfigAction,
    },
    /// [Mutates] Install cmx's own companion agent skill (global scope by default).
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
        /// Output formatting flags.
        output: OutputArgs,
    },
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser, error::ErrorKind};
    use clap_complete::Shell;

    use super::{Cli, Commands, HomeAction, OutputArgs};

    fn rendered_help(args: &[&str]) -> String {
        let err = Cli::command()
            .try_get_matches_from_mut(args)
            .expect_err("help flag should short-circuit parsing");
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        err.to_string()
    }

    fn top_level_help() -> String {
        rendered_help(&["cmx", "--help"])
    }

    fn help_for(path: &[&str]) -> String {
        let mut args = Vec::with_capacity(path.len() + 2);
        args.push("cmx");
        args.extend_from_slice(path);
        args.push("--help");
        rendered_help(&args)
    }

    #[test]
    fn top_level_help_keeps_full_platform_roster() {
        let help = top_level_help();
        assert!(help.contains("Platform values:"), "{help}");
        assert!(help.contains("opencode   opencode — markdown agents"), "{help}");
        assert!(help.contains("codex      Codex CLI — TOML agents"), "{help}");
    }

    #[test]
    fn subcommand_help_uses_compact_platform_line() {
        let help = help_for(&["source", "add"]);
        assert!(help.contains("--platform <PLATFORM>"), "{help}");
        assert!(help.contains("see 'cmx --help' for the full list"), "{help}");
        assert!(help.contains("CMX_PLATFORM"), "{help}");
        assert!(!help.contains("opencode — markdown agents"), "{help}");
        assert!(!help.contains("Codex CLI — TOML agents"), "{help}");
        assert!(!help.contains("Possible values:"), "{help}");
        assert!(help.lines().count() < 25, "{help}");
    }

    #[test]
    fn invalid_platform_values_still_list_possible_values() {
        let err = Cli::try_parse_from(["cmx", "list", "--platform", "bogus"])
            .err()
            .expect("invalid platform should be rejected")
            .to_string();
        assert!(err.contains("possible values"), "{err}");
        assert!(err.contains("claude"), "{err}");
        assert!(err.contains("codex"), "{err}");
        assert!(err.contains("devin"), "{err}");
    }

    #[test]
    fn invalid_completion_shell_values_list_possible_values() {
        let err = Cli::try_parse_from(["cmx", "completions", "bogus"])
            .err()
            .expect("invalid shell should be rejected")
            .to_string();
        assert!(err.contains("possible values"), "{err}");
        assert!(err.contains("bash"), "{err}");
        assert!(err.contains("zsh"), "{err}");
        assert!(err.contains("fish"), "{err}");
        assert!(err.contains("elvish"), "{err}");
        assert!(err.contains("powershell"), "{err}");
    }

    #[test]
    fn parse_completions() {
        let cli = Cli::try_parse_from(["cmx", "completions", "zsh"]).unwrap();
        assert!(matches!(cli.command, Commands::Completions { shell: Shell::Zsh }));
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
