use crate::error::Result;
use std::fmt::Write as _;
use std::path::Path;

use crate::context::AppContext;
use crate::fs_util;
use crate::text_diff::{Op, lcs_ops, render_hunks, split_lines};
use crate::types::{self, ArtifactKind};

use super::{FileChange, FileStatus};

/// The structural diff of an artifact: a per-file summary plus a directional
/// unified diff (`−` source, `+` installed).
pub(super) struct ArtifactDiff {
    pub(super) changes: Vec<FileChange>,
    pub(super) unified: String,
}

/// Produce a directional diff between an installed artifact and its source
/// counterpart, dispatching to the correct strategy (file diff for agents,
/// directory diff for skills). `src_label` names the source side (e.g. `home`).
pub(super) fn diff_artifact(
    kind: ArtifactKind,
    installed: &Path,
    source: &Path,
    src_label: &str,
    ctx: &AppContext<'_>,
) -> Result<ArtifactDiff> {
    match kind {
        ArtifactKind::Agent => diff_files(installed, source, src_label, ctx),
        ArtifactKind::Skill => diff_dirs(installed, source, src_label, ctx),
    }
}

pub(crate) fn file_changes_between(
    kind: ArtifactKind,
    current: &Path,
    desired: &Path,
    ctx: &AppContext<'_>,
) -> Result<Vec<FileChange>> {
    Ok(match kind {
        ArtifactKind::Agent => {
            if ctx.fs.exists(current) && ctx.fs.exists(desired) {
                diff_files(current, desired, "desired", ctx)?.changes
            } else if ctx.fs.exists(current) {
                single_agent_changes(current, FileStatus::OnlyInInstalled, ctx)?
            } else if ctx.fs.exists(desired) {
                single_agent_changes(desired, FileStatus::OnlyInSource, ctx)?
            } else {
                Vec::new()
            }
        }
        ArtifactKind::Skill => {
            if ctx.fs.exists(current) && ctx.fs.exists(desired) {
                diff_dirs(current, desired, "desired", ctx)?.changes
            } else if ctx.fs.exists(current) {
                single_dir_changes(current, FileStatus::OnlyInInstalled, ctx)?
            } else if ctx.fs.exists(desired) {
                single_dir_changes(desired, FileStatus::OnlyInSource, ctx)?
            } else {
                Vec::new()
            }
        }
    })
}

fn single_agent_changes(
    path: &Path,
    status: FileStatus,
    ctx: &AppContext<'_>,
) -> Result<Vec<FileChange>> {
    let content = ctx.fs.read_to_string(path)?;
    let lines = split_lines(&content);
    Ok(vec![FileChange {
        path: path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string(),
        status,
        added: usize::from(matches!(status, FileStatus::OnlyInInstalled)) * lines.len(),
        removed: usize::from(matches!(status, FileStatus::OnlyInSource)) * lines.len(),
    }])
}

fn single_dir_changes(
    dir: &Path,
    status: FileStatus,
    ctx: &AppContext<'_>,
) -> Result<Vec<FileChange>> {
    let mut changes = Vec::new();
    for path in collect_relative_files_with(dir, ctx)? {
        let content = ctx.fs.read_to_string(&dir.join(&path))?;
        let lines = split_lines(&content);
        changes.push(FileChange {
            path,
            status,
            added: usize::from(matches!(status, FileStatus::OnlyInInstalled)) * lines.len(),
            removed: usize::from(matches!(status, FileStatus::OnlyInSource)) * lines.len(),
        });
    }
    Ok(changes)
}

pub(super) fn diff_files(
    installed: &Path,
    source: &Path,
    src_label: &str,
    ctx: &AppContext<'_>,
) -> Result<ArtifactDiff> {
    let installed_content = ctx.fs.read_to_string(installed)?;
    let source_content = ctx.fs.read_to_string(source)?;
    let name = installed.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();

    let mut changes = Vec::new();
    let mut unified = String::new();
    if let Some((change, block)) =
        modified_file_block(&name, src_label, &source_content, &installed_content)
    {
        changes.push(change);
        unified.push_str(&block);
    }
    Ok(ArtifactDiff { changes, unified })
}

pub(super) fn diff_dirs(
    installed: &Path,
    source: &Path,
    src_label: &str,
    ctx: &AppContext<'_>,
) -> Result<ArtifactDiff> {
    let installed_files = collect_relative_files_with(installed, ctx)?;
    let source_files = collect_relative_files_with(source, ctx)?;

    let mut all: Vec<&String> = installed_files.iter().chain(source_files.iter()).collect();
    all.sort();
    all.dedup();

    let mut changes = Vec::new();
    let mut unified = String::new();

    for f in all {
        let in_installed = installed_files.contains(f);
        let in_source = source_files.contains(f);
        match (in_installed, in_source) {
            (true, true) => {
                let i_content = ctx.fs.read_to_string(&installed.join(f))?;
                let s_content = ctx.fs.read_to_string(&source.join(f))?;
                if let Some((change, block)) =
                    modified_file_block(f, src_label, &s_content, &i_content)
                {
                    changes.push(change);
                    let _ = writeln!(unified, "{block}");
                }
            }
            (true, false) => {
                let content = ctx.fs.read_to_string(&installed.join(f))?;
                let lines = split_lines(&content);
                changes.push(FileChange {
                    path: f.clone(),
                    status: FileStatus::OnlyInInstalled,
                    added: lines.len(),
                    removed: 0,
                });
                let mut block = format!("+++ installed/{f}  (new file)\n");
                for l in &lines {
                    let _ = writeln!(block, "  + {l}");
                }
                let _ = writeln!(unified, "{block}");
            }
            (false, true) => {
                let content = ctx.fs.read_to_string(&source.join(f))?;
                let lines = split_lines(&content);
                changes.push(FileChange {
                    path: f.clone(),
                    status: FileStatus::OnlyInSource,
                    added: 0,
                    removed: lines.len(),
                });
                let mut block = format!("--- {src_label}/{f}  (removed locally)\n");
                for l in &lines {
                    let _ = writeln!(block, "  - {l}");
                }
                let _ = writeln!(unified, "{block}");
            }
            (false, false) => unreachable!("name came from the union of both sides"),
        }
    }

    Ok(ArtifactDiff { changes, unified })
}

/// Build the change summary and unified-diff block for one file present on both
/// sides. Returns `None` when the content is identical.
pub(super) fn modified_file_block(
    path: &str,
    src_label: &str,
    source: &str,
    installed: &str,
) -> Option<(FileChange, String)> {
    let old = split_lines(source);
    let new = split_lines(installed);
    let ops = lcs_ops(&old, &new);
    let added = ops.iter().filter(|(o, _)| *o == Op::Ins).count();
    let removed = ops.iter().filter(|(o, _)| *o == Op::Del).count();
    if added == 0 && removed == 0 {
        return None;
    }
    let block = format!("--- {src_label}/{path}\n+++ installed/{path}\n{}", render_hunks(&ops, 3));
    Some((
        FileChange {
            path: path.to_string(),
            status: FileStatus::Modified,
            added,
            removed,
        },
        block,
    ))
}

pub(super) fn collect_relative_files_with(dir: &Path, ctx: &AppContext<'_>) -> Result<Vec<String>> {
    let mut files = fs_util::collect_files_recursive(dir, ctx.fs)?
        .into_iter()
        .map(|p| types::relative_path_string(&p, dir))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestContext;
    use crate::types::ArtifactKind;

    // --- modified_file_block ---

    #[test]
    fn modified_file_block_directional_headers_and_counts() {
        let (change, block) =
            modified_file_block("SKILL.md", "home", "line one\nold\n", "line one\nnew\n").unwrap();
        assert_eq!(change.status, FileStatus::Modified);
        assert_eq!((change.added, change.removed), (1, 1));
        assert!(block.contains("--- home/SKILL.md"), "source header: {block}");
        assert!(block.contains("+++ installed/SKILL.md"), "installed header: {block}");
        assert!(block.contains("- old"), "minus is source: {block}");
        assert!(block.contains("+ new"), "plus is installed: {block}");
    }

    #[test]
    fn modified_file_block_none_when_identical() {
        assert!(modified_file_block("SKILL.md", "home", "same\n", "same\n").is_none());
    }

    // --- collect_relative_files_with ---

    #[test]
    fn collect_relative_files_returns_sorted_relative_paths() {
        let t = TestContext::new();

        t.fs.add_file("/dir/b.md", "b");
        t.fs.add_file("/dir/a.md", "a");
        t.fs.add_file("/dir/sub/c.md", "c");

        let ctx = t.ctx();
        let result = collect_relative_files_with(std::path::Path::new("/dir"), &ctx).unwrap();

        assert_eq!(result, vec!["a.md", "b.md", "sub/c.md"]);
    }

    // --- diff_files (agents) ---

    #[test]
    fn diff_files_builds_directional_diff() {
        let t = TestContext::new();
        t.fs.add_file("/installed/agent.md", "shared\ninstalled line\n");
        t.fs.add_file("/source/agent.md", "shared\nsource line\n");

        let ctx = t.ctx();
        let d = diff_files(
            std::path::Path::new("/installed/agent.md"),
            std::path::Path::new("/source/agent.md"),
            "home",
            &ctx,
        )
        .unwrap();

        assert_eq!(d.changes.len(), 1);
        assert_eq!(d.changes[0].status, FileStatus::Modified);
        assert!(d.unified.contains("- source line"), "source is minus: {}", d.unified);
        assert!(d.unified.contains("+ installed line"), "installed is plus: {}", d.unified);
    }

    #[test]
    fn diff_files_errors_on_missing_installed() {
        let t = TestContext::new();
        t.fs.add_file("/source/agent.md", "source content");

        let ctx = t.ctx();
        let result = diff_files(
            std::path::Path::new("/installed/agent.md"),
            std::path::Path::new("/source/agent.md"),
            "home",
            &ctx,
        );

        assert!(result.is_err());
    }

    // --- diff_dirs (skills) ---

    #[test]
    fn diff_dirs_identical_directories_returns_no_changes() {
        let t = TestContext::new();
        let content = "---\ndescription: My skill\n---\n";
        t.fs.add_file("/installed/my-skill/SKILL.md", content);
        t.fs.add_file("/source/my-skill/SKILL.md", content);

        let ctx = t.ctx();
        let d = diff_dirs(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            "home",
            &ctx,
        )
        .unwrap();

        assert!(d.changes.is_empty(), "no changes for identical dirs");
        assert!(d.unified.is_empty());
    }

    #[test]
    fn diff_dirs_flags_file_only_in_installed() {
        let t = TestContext::new();
        t.fs.add_file("/installed/my-skill/SKILL.md", "skill");
        t.fs.add_file("/installed/my-skill/extra.md", "extra\nlines\n");
        t.fs.add_file("/source/my-skill/SKILL.md", "skill");

        let ctx = t.ctx();
        let d = diff_dirs(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            "home",
            &ctx,
        )
        .unwrap();

        let extra = d.changes.iter().find(|c| c.path == "extra.md").expect("extra.md change");
        assert_eq!(extra.status, FileStatus::OnlyInInstalled);
        assert!(d.unified.contains("+++ installed/extra.md  (new file)"), "{}", d.unified);
    }

    #[test]
    fn diff_dirs_flags_file_only_in_source() {
        let t = TestContext::new();
        t.fs.add_file("/installed/my-skill/SKILL.md", "skill");
        t.fs.add_file("/source/my-skill/SKILL.md", "skill");
        t.fs.add_file("/source/my-skill/gone.md", "removed\n");

        let ctx = t.ctx();
        let d = diff_dirs(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            "home",
            &ctx,
        )
        .unwrap();

        let gone = d.changes.iter().find(|c| c.path == "gone.md").expect("gone.md change");
        assert_eq!(gone.status, FileStatus::OnlyInSource);
        assert!(d.unified.contains("--- home/gone.md  (removed locally)"), "{}", d.unified);
    }

    // --- diff_artifact dispatch ---

    #[test]
    fn diff_artifact_dispatches_to_diff_files_for_agent() {
        let t = TestContext::new();
        t.fs.add_file("/installed/agent.md", "shared\ninstalled line\n");
        t.fs.add_file("/source/agent.md", "shared\nsource line\n");

        let ctx = t.ctx();
        let d = diff_artifact(
            ArtifactKind::Agent,
            std::path::Path::new("/installed/agent.md"),
            std::path::Path::new("/source/agent.md"),
            "home",
            &ctx,
        )
        .unwrap();

        assert_eq!(d.changes.len(), 1);
    }
}
