use std::io::{self, Write};

use clap::CommandFactory;
use clap_complete::{Shell, generate};

use crate::cli::Cli;

pub fn generate_to(shell: Shell, mut writer: impl Write) -> io::Result<()> {
    let mut command = Cli::command();
    generate(shell, &mut command, "cmx", &mut writer);
    writer.flush()
}
