use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::context::AppContext;
use crate::gateway::Filesystem;
use crate::types::ArtifactKind;

/// Copy an artifact from source to destination, returning the destination path.
pub(crate) fn copy_artifact(
    artifact_path: &Path,
    dest_dir: &Path,
    kind: ArtifactKind,
    artifact_name: &str,
    ctx: &AppContext<'_>,
) -> Result<PathBuf> {
    let dest_path = match kind {
        ArtifactKind::Agent => {
            let filename = artifact_path.file_name().context("Invalid agent path")?;
            let dest = dest_dir.join(filename);
            ctx.fs.copy_file(artifact_path, &dest).with_context(|| {
                format!("Failed to copy {} to {}", artifact_path.display(), dest.display())
            })?;
            dest
        }
        ArtifactKind::Skill => {
            let dir_name = artifact_path.file_name().context("Invalid skill path")?;
            let dest = dest_dir.join(dir_name);
            copy_dir_recursive_with(artifact_path, &dest, ctx.fs)?;
            dest
        }
    };

    // Validate skill installation
    if matches!(kind, ArtifactKind::Skill) {
        let skill_md = dest_path.join("SKILL.md");
        if !ctx.fs.exists(&skill_md) {
            let _ = ctx.fs.remove_dir_all(&dest_path);
            bail!("Skill '{artifact_name}' is missing SKILL.md. Partial install removed.");
        }
    }

    Ok(dest_path)
}

pub(crate) fn copy_dir_recursive_with(src: &Path, dest: &Path, fs: &dyn Filesystem) -> Result<()> {
    fs.create_dir_all(dest)
        .with_context(|| format!("Failed to create {}", dest.display()))?;

    for entry in fs.read_dir(src)? {
        let dest_path = dest.join(&entry.file_name);

        if entry.is_dir {
            copy_dir_recursive_with(&entry.path, &dest_path, fs)?;
        } else {
            fs.copy_file(&entry.path, &dest_path)?;
        }
    }

    Ok(())
}
