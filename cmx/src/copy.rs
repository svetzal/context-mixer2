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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{make_ctx, test_paths};
    use chrono::Utc;

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
        fs.add_file("/src/SKILL.md", "# skill");
        fs.add_file("/src/subdir/tool.py", "code");

        copy_dir_recursive_with(Path::new("/src"), Path::new("/dest"), &fs).unwrap();

        assert!(fs.file_exists(Path::new("/dest/SKILL.md")));
        assert!(fs.file_exists(Path::new("/dest/subdir/tool.py")));
    }

    #[test]
    fn copy_dir_recursive_with_fails_on_unreadable_source() {
        let fs = FakeFilesystem::new();
        let result = copy_dir_recursive_with(Path::new("/nonexistent"), Path::new("/dest"), &fs);
        assert!(result.is_err());
    }

    #[test]
    fn copy_artifact_skill_with_skill_md_succeeds() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/source/my-skill/SKILL.md", "---\ndescription: A skill\n---\n");
        fs.add_file("/source/my-skill/tool.py", "code");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
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
        assert!(fs.file_exists(&dest_path.join("SKILL.md")));
    }

    #[test]
    fn copy_artifact_skill_missing_skill_md_returns_error_and_cleans_up() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/source/my-skill/tool.py", "code");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
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
            !fs.file_exists(Path::new("/dest/my-skill/tool.py")),
            "partial install should be removed"
        );
    }

    #[test]
    fn copy_artifact_agent_does_not_require_skill_md() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file(
            "/source/agents/my-agent.md",
            "---\nname: my-agent\ndescription: An agent\n---\n",
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = copy_artifact(
            Path::new("/source/agents/my-agent.md"),
            Path::new("/dest/agents"),
            ArtifactKind::Agent,
            "my-agent",
            &ctx,
        );

        assert!(result.is_ok(), "agent copy should succeed without SKILL.md: {:?}", result.err());
        assert!(fs.file_exists(Path::new("/dest/agents/my-agent.md")));
    }
}
