use anyhow::{Context as _, Result, bail};
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
    let dest_path = kind.copy_to(artifact_path, dest_dir, ctx.fs)?;

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
