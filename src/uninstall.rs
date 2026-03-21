use anyhow::{Result, bail};
use std::fs;

use crate::config;
use crate::lockfile;
use crate::types::ArtifactKind;

pub fn uninstall(name: &str, kind: ArtifactKind, local: bool) -> Result<()> {
    let dir = config::install_dir(kind, local)?;

    let target = kind.installed_path(name, &dir);

    if !target.exists() {
        let scope = if local { "local" } else { "global" };
        bail!("No {kind} named '{name}' found in {scope} scope.");
    }

    // Remove from disk
    match kind {
        ArtifactKind::Agent => {
            fs::remove_file(&target)?;
        }
        ArtifactKind::Skill => {
            fs::remove_dir_all(&target)?;
        }
    }

    // Remove from lock file
    let mut lock = lockfile::load(local)?;
    let had_entry = lock.packages.remove(name).is_some();
    lockfile::save(&lock, local)?;

    let scope = if local { "local" } else { "global" };
    println!("Uninstalled {name} ({kind}) from {scope} scope.");
    if !had_entry {
        println!("  (no lock file entry found — artifact was untracked)");
    }

    Ok(())
}
