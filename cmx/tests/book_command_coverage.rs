/// Book documentation-coverage drift guard.
///
/// Mirrors the enforcement pattern in `architecture_doc.rs`, but for the
/// mdBook command reference instead of `AGENTS.md`'s architecture map: every
/// top-level `cmx` command noun, and every `cmx set` subcommand, must appear
/// as a literal string somewhere in `book/src/reference/commands.md`.
///
/// The command names are read straight from the `clap` `Command` tree (via
/// `CommandFactory`), not hand-copied, so this test can't itself drift from
/// the actual CLI surface — only the book can.
///
/// This is what actually prevents recurrence: the `cmx set` family shipped
/// 3,630 lines of implementation with zero book coverage because nothing
/// failed when it landed undocumented.
use clap::CommandFactory;
use cmx::cli::Cli;
use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cmx crate must have a parent workspace directory")
        .to_path_buf()
}

fn commands_md_content() -> String {
    let path = workspace_root().join("book/src/reference/commands.md");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e))
}

#[test]
fn every_top_level_command_is_documented() {
    let content = commands_md_content();
    let root = Cli::command();

    let undocumented: Vec<&str> = root
        .get_subcommands()
        .map(clap::Command::get_name)
        .filter(|name| !content.contains(name))
        .collect();

    assert!(
        undocumented.is_empty(),
        "book/src/reference/commands.md is missing coverage for top-level command(s): {undocumented:?}\n\
         Add a section (or table row) documenting each one."
    );
}

#[test]
fn every_set_subcommand_is_documented() {
    let content = commands_md_content();
    let root = Cli::command();

    let set_cmd = root.find_subcommand("set").expect(
        "Cli must have a top-level 'set' subcommand — this test needs updating if it was renamed",
    );

    let undocumented: Vec<&str> = set_cmd
        .get_subcommands()
        .map(clap::Command::get_name)
        .filter(|name| !content.contains(name))
        .collect();

    assert!(
        undocumented.is_empty(),
        "book/src/reference/commands.md is missing coverage for 'cmx set' subcommand(s): {undocumented:?}\n\
         Add each one to the ## Sets section's grammar block."
    );
}
