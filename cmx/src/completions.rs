//! Shell-completion generation for the `cmx` CLI.

use std::io::{self, Write};

use clap::CommandFactory;
use clap_complete::{Shell, generate};

use crate::cli::Cli;

/// Generates shell-completion script text for the given `shell` and writes it
/// to `writer`, flushing once the full script has been emitted.
pub fn generate_to(shell: Shell, mut writer: impl Write) -> io::Result<()> {
    let mut command = Cli::command();
    generate(shell, &mut command, "cmx", &mut writer);
    writer.flush()
}
