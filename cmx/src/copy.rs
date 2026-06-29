use anyhow::{Context as _, Result, bail};
use std::path::{Path, PathBuf};

use crate::context::AppContext;
use crate::gateway::Filesystem;
use crate::types::ArtifactKind;

/// Copy an artifact from `source` into `dest_dir`, dispatching to the correct
/// strategy (file copy for agents, recursive directory copy for skills).
/// Returns the destination path.
pub(crate) fn copy_artifact_to(
    kind: ArtifactKind,
    source: &Path,
    dest_dir: &Path,
    fs: &dyn Filesystem,
) -> anyhow::Result<PathBuf> {
    let name = source
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid source path: {}", source.display()))?;
    let dest = dest_dir.join(name);
    match kind {
        ArtifactKind::Agent => {
            fs.copy_file(source, &dest).with_context(|| {
                format!("Failed to copy {} to {}", source.display(), dest.display())
            })?;
        }
        ArtifactKind::Skill => {
            copy_dir_recursive_with(source, &dest, fs)?;
        }
    }
    Ok(dest)
}

/// Copy an artifact from source to destination, returning the destination path.
///
/// Most artifacts are copied verbatim. Codex agents are the exception: the
/// source markdown is transformed into a codex subagent TOML document (see
/// [`crate::codex_agent`]) and written as `<name>.toml`.
pub(crate) fn copy_artifact(
    artifact_path: &Path,
    dest_dir: &Path,
    kind: ArtifactKind,
    artifact_name: &str,
    ctx: &AppContext<'_>,
) -> Result<PathBuf> {
    if kind == ArtifactKind::Agent && ctx.paths.platform.transforms_agent_to_toml() {
        return transform_agent_to_codex_toml(artifact_path, dest_dir, artifact_name, ctx);
    }

    let dest_path = copy_artifact_to(kind, artifact_path, dest_dir, ctx.fs)?;

    if matches!(kind, ArtifactKind::Skill) {
        let skill_md = dest_path.join("SKILL.md");
        if !ctx.fs.exists(&skill_md) {
            let _ = ctx.fs.remove_dir_all(&dest_path);
            bail!("Skill '{artifact_name}' is missing SKILL.md. Partial install removed.");
        }
    }

    Ok(dest_path)
}

/// Read a markdown agent, transform it into codex subagent TOML, and write it to
/// `<dest_dir>/<name>.toml`. Returns the written path.
fn transform_agent_to_codex_toml(
    source: &Path,
    dest_dir: &Path,
    name: &str,
    ctx: &AppContext<'_>,
) -> Result<PathBuf> {
    let markdown = ctx
        .fs
        .read_to_string(source)
        .with_context(|| format!("Failed to read agent source {}", source.display()))?;
    let toml = crate::codex_agent::markdown_to_codex_toml(&markdown, name);

    ctx.fs.create_dir_all(dest_dir)?;
    let dest = dest_dir.join(format!("{name}.toml"));
    ctx.fs
        .write(&dest, &toml)
        .with_context(|| format!("Failed to write codex agent {}", dest.display()))?;
    Ok(dest)
}

pub(crate) fn copy_dir_recursive_with(src: &Path, dest: &Path, fs: &dyn Filesystem) -> Result<()> {
    fs.create_dir_all(dest)?;

    for entry in fs.read_dir(src)? {
        // Skip transient/generated content (node_modules, __pycache__, …) so the
        // canonical home and projected installs don't accumulate vendored deps or
        // build artifacts. Mirrors the checksum collector's ignore set, keeping a
        // copied skill identical to its checksummed identity.
        if crate::fs_util::is_transient(&entry.file_name) {
            continue;
        }

        let dest_path = dest.join(&entry.file_name);

        if entry.is_dir {
            copy_dir_recursive_with(&entry.path, &dest_path, fs)?;
        } else {
            fs.copy_file(&entry.path, &dest_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{TestContext, add_skill, skill_content};

    #[test]
    fn copy_dir_recursive_with_copies_flat_directory() {
        let fs = FakeFilesystem::new();
        fs.add_file("/src/alpha.md", "# alpha");
        fs.add_file("/src/beta.md", "# beta");

        copy_dir_recursive_with(Path::new("/src"), Path::new("/dest"), &fs).unwrap();

        assert!(fs.file_exists(Path::new("/dest/alpha.md")));
        assert!(fs.file_exists(Path::new("/dest/beta.md")));
    }

    #[test]
    fn copy_dir_recursive_with_copies_nested_directories() {
        let fs = FakeFilesystem::new();
        fs.add_file("/src/SKILL.md", skill_content(""));
        fs.add_file("/src/subdir/tool.py", "code");

        copy_dir_recursive_with(Path::new("/src"), Path::new("/dest"), &fs).unwrap();

        assert!(fs.file_exists(Path::new("/dest/SKILL.md")));
        assert!(fs.file_exists(Path::new("/dest/subdir/tool.py")));
    }

    #[test]
    fn copy_dir_recursive_with_skips_transient_content() {
        let fs = FakeFilesystem::new();
        fs.add_file("/src/SKILL.md", skill_content(""));
        fs.add_file("/src/scripts/tool.mjs", "code");
        fs.add_file("/src/scripts/package.json", "{}");
        fs.add_file("/src/scripts/node_modules/dep/index.js", "vendored");
        fs.add_file("/src/scripts/__pycache__/tool.cpython-312.pyc", "bytecode");

        copy_dir_recursive_with(Path::new("/src"), Path::new("/dest"), &fs).unwrap();

        // Authored content is copied.
        assert!(fs.file_exists(Path::new("/dest/SKILL.md")));
        assert!(fs.file_exists(Path::new("/dest/scripts/tool.mjs")));
        assert!(fs.file_exists(Path::new("/dest/scripts/package.json")));
        // Transient content is not.
        assert!(
            !fs.file_exists(Path::new("/dest/scripts/node_modules/dep/index.js")),
            "node_modules must not be copied"
        );
        assert!(
            !fs.file_exists(Path::new("/dest/scripts/__pycache__/tool.cpython-312.pyc")),
            "__pycache__ must not be copied"
        );
    }

    #[test]
    fn copy_dir_recursive_with_fails_on_unreadable_source() {
        let fs = FakeFilesystem::new();
        let result = copy_dir_recursive_with(Path::new("/nonexistent"), Path::new("/dest"), &fs);
        assert!(result.is_err());
    }

    #[test]
    fn copy_artifact_skill_with_skill_md_succeeds() {
        let t = TestContext::new();

        add_skill(&t.fs, "/source", "my-skill", "A skill");
        t.fs.add_file("/source/my-skill/tool.py", "code");

        let ctx = t.ctx();
        let dest_dir = Path::new("/dest");
        let result = copy_artifact(
            Path::new("/source/my-skill"),
            dest_dir,
            ArtifactKind::Skill,
            "my-skill",
            &ctx,
        );

        assert!(result.is_ok(), "expected ok, got: {:?}", result.err());
        let dest_path = result.unwrap();
        assert!(t.fs.file_exists(&dest_path.join("SKILL.md")));
    }

    #[test]
    fn copy_artifact_skill_missing_skill_md_returns_error_and_cleans_up() {
        let t = TestContext::new();

        t.fs.add_file("/source/my-skill/tool.py", "code");

        let ctx = t.ctx();
        let result = copy_artifact(
            Path::new("/source/my-skill"),
            Path::new("/dest"),
            ArtifactKind::Skill,
            "my-skill",
            &ctx,
        );

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing SKILL.md"), "unexpected: {msg}");
        assert!(
            !t.fs.file_exists(Path::new("/dest/my-skill/tool.py")),
            "partial install should be removed"
        );
    }

    #[test]
    fn copy_artifact_agent_does_not_require_skill_md() {
        let t = TestContext::new();

        t.fs.add_file(
            "/source/agents/my-agent.md",
            "---\nname: my-agent\ndescription: An agent\n---\n",
        );

        let ctx = t.ctx();
        let result = copy_artifact(
            Path::new("/source/agents/my-agent.md"),
            Path::new("/dest/agents"),
            ArtifactKind::Agent,
            "my-agent",
            &ctx,
        );

        assert!(result.is_ok(), "agent copy should succeed without SKILL.md: {:?}", result.err());
        assert!(t.fs.file_exists(Path::new("/dest/agents/my-agent.md")));
    }
}
