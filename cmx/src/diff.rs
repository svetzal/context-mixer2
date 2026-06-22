use anyhow::{Context, Result, bail};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::fs_util;
use crate::lockfile;
use crate::source_iter;
use crate::types::{self, ArtifactKind};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DiffOutput {
    pub artifact_name: String,
    pub kind: ArtifactKind,
    pub is_up_to_date: bool,
    /// Where the installed copy lives (the side `+` lines come from).
    pub installed_path: PathBuf,
    pub installed_version: Option<String>,
    /// `true` when the installed copy was edited after install (its bytes no
    /// longer match the lock's recorded checksum).
    pub installed_locally_edited: bool,
    /// Where the source copy lives (the side `−` lines come from).
    pub source_path: PathBuf,
    pub source_version: Option<String>,
    pub source_name: String,
    /// Per-file summary of what differs, so the direction of each change is
    /// legible without reading the whole diff.
    pub file_changes: Vec<FileChange>,
    pub diff_text: Option<String>,
    pub analysis: Option<String>,
    /// The reconciliation directions to offer — both ways, since `diff` can't
    /// know which side is authoritative.
    pub reconciliations: Vec<Reconciliation>,
    /// When `true`, render the full line-by-line unified diff; otherwise the
    /// output stays compact (summary + analysis) with a hint to pass `--full`.
    pub show_full: bool,
}

/// How one file differs between the source and installed copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Present on both sides with differing content.
    Modified,
    /// Present only in the installed copy (added locally).
    OnlyInInstalled,
    /// Present only in the source copy (removed locally).
    OnlyInSource,
}

/// One file's change summary. `added` counts lines present only in the installed
/// copy (`+`); `removed` counts lines present only in the source copy (`−`).
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    pub added: usize,
    pub removed: usize,
}

/// One way to reconcile the difference: a human-readable direction plus the
/// exact command, with an optional caveat.
#[derive(Debug, Clone)]
pub struct Reconciliation {
    pub description: String,
    pub command: String,
    pub note: Option<String>,
}

/// The structural diff of an artifact: a per-file summary plus a directional
/// unified diff (`−` source, `+` installed).
struct ArtifactDiff {
    changes: Vec<FileChange>,
    unified: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn diff(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<DiffOutput> {
    let mut output = gather_diff_with(name, kind, ctx).await?;
    output.show_full = full;
    Ok(output)
}

// ---------------------------------------------------------------------------
// Gather (no println!)
// ---------------------------------------------------------------------------

pub(crate) async fn gather_diff_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<DiffOutput> {
    // Find the installed file on disk (global then local)
    let (installed_path, local) = config::find_installed_path(name, kind, ctx.fs, ctx.paths)
        .with_context(|| format!("No installed {kind} named '{name}' found on disk."))?;

    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources_with(name, kind, ctx)?;

    // Compare checksums
    let installed_checksum = checksum::checksum_artifact(&installed_path, kind, ctx.fs)?;
    let source_checksum = checksum::checksum_artifact(&source_path, kind, ctx.fs)?;

    // disk-vs-source axis: answers "do the bytes differ right now?", distinct from source-vs-lock "outdated" rule
    if installed_checksum == source_checksum {
        return Ok(DiffOutput {
            artifact_name: name.to_string(),
            kind,
            is_up_to_date: true,
            installed_path,
            installed_version: None,
            installed_locally_edited: false,
            source_path,
            source_version,
            source_name,
            file_changes: Vec::new(),
            diff_text: None,
            analysis: None,
            reconciliations: Vec::new(),
            show_full: false,
        });
    }

    // Get installed version from lock file if available
    let lock = lockfile::load(local, ctx.fs, ctx.paths)?;
    let installed_version = lock.packages.get(name).and_then(|e| e.version.clone());

    // The installed copy differs from source. It is "locally edited" when its
    // bytes no longer match the lock's recorded checksum (so re-installing from
    // source would need `--force` to overwrite the edits).
    let locally_modified = lock
        .packages
        .get(name)
        .is_some_and(|e| e.installed_checksum != installed_checksum);

    let reconciliations = reconciliations(name, kind, &source_name, locally_modified);

    // Build the directional, file-level diff.
    let dir_diff = diff_artifact(kind, &installed_path, &source_path, &source_name, ctx)?;

    let installed_ver_display = installed_version.as_deref().unwrap_or("unversioned");
    let source_ver_display = source_version.as_deref().unwrap_or("unversioned");

    let system_prompt = "You are a technical analyst comparing two versions of an AI coding assistant artifact (an agent definition or skill definition written in markdown). \
        You are given a unified diff where lines prefixed with `-` come from the SOURCE copy and lines prefixed with `+` come from the INSTALLED copy. \
        Provide a clear, concise summary of the differences. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed (and on which side)\n\
        2. Whether the update is significant or cosmetic\n\
        3. A recommendation: which copy looks more authoritative, and which direction to reconcile\n\n\
        Keep your analysis brief and actionable — a few paragraphs at most.";

    let user_prompt = format!(
        "Compare these two versions of the {kind} '{name}':\n\
        - Installed copy (the `+` side): {installed_ver_display}\n\
        - Source copy from '{source_name}' (the `−` side): {source_ver_display}\n\n\
        {}",
        dir_diff.unified
    );

    let analysis = match ctx.llm {
        Some(llm) => llm.analyze(system_prompt, &user_prompt).await?,
        None => bail!("LLM client not configured for diff analysis"),
    };

    Ok(DiffOutput {
        artifact_name: name.to_string(),
        kind,
        is_up_to_date: false,
        installed_path,
        installed_version,
        installed_locally_edited: locally_modified,
        source_path,
        source_version,
        source_name,
        file_changes: dir_diff.changes,
        diff_text: Some(dir_diff.unified),
        analysis: Some(analysis),
        reconciliations,
        show_full: false,
    })
}

/// Build the reconciliation directions. When the source is the canonical home,
/// the installed edits can be promoted into it; either way they can be discarded
/// by re-installing from source. `diff` never picks for the user.
fn reconciliations(
    name: &str,
    kind: ArtifactKind,
    source_name: &str,
    locally_modified: bool,
) -> Vec<Reconciliation> {
    let mut out = Vec::new();
    let source_is_home = source_name == crate::adopt::HOME_SOURCE;

    if source_is_home {
        out.push(Reconciliation {
            description: "keep the installed edits, update the home".to_string(),
            command: format!("cmx {kind} promote {name}"),
            note: None,
        });
    }

    let restore_target = if source_is_home {
        "home".to_string()
    } else {
        format!("{source_name} copy")
    };
    out.push(Reconciliation {
        description: format!("discard the installed edits, restore the {restore_target}"),
        command: if locally_modified {
            format!("cmx {kind} update {name} --force")
        } else {
            format!("cmx {kind} update {name}")
        },
        note: locally_modified.then(|| "--force overwrites the installed local edits".to_string()),
    });
    out
}

fn find_in_sources_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, String, Option<String>)> {
    if let Some(sa) = source_iter::find_by_name_and_kind(name, kind, ctx)?.into_iter().next() {
        return Ok((sa.artifact.path, sa.source_name, sa.artifact.version));
    }
    bail!("No {kind} named '{name}' found in any registered source.");
}

/// Produce a directional diff between an installed artifact and its source
/// counterpart, dispatching to the correct strategy (file diff for agents,
/// directory diff for skills). `src_label` names the source side (e.g. `home`).
fn diff_artifact(
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

fn diff_files(
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

fn diff_dirs(
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
                let i_content = ctx
                    .fs
                    .read_to_string(&installed.join(f))
                    .with_context(|| format!("Failed to read installed file {f}"))?;
                let s_content = ctx
                    .fs
                    .read_to_string(&source.join(f))
                    .with_context(|| format!("Failed to read source file {f}"))?;
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
fn modified_file_block(
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

fn collect_relative_files_with(dir: &Path, ctx: &AppContext<'_>) -> Result<Vec<String>> {
    let mut files = fs_util::collect_files_recursive(dir, ctx.fs)?
        .into_iter()
        .map(|p| types::relative_path_string(&p, dir))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

// ---------------------------------------------------------------------------
// Line diff (self-contained LCS — no external crate)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Del,
    Ins,
}

fn split_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.lines().collect()
    }
}

/// Longest-common-subsequence line diff. Returns ops in order: `Del` lines come
/// from `old` (the source/`−` side), `Ins` from `new` (the installed/`+` side).
/// Falls back to a whole-file replace for pathologically large inputs to bound
/// the O(n·m) table.
fn lcs_ops<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Op, &'a str)> {
    let (n, m) = (old.len(), new.len());
    if n.saturating_mul(m) > 4_000_000 {
        let mut ops = Vec::with_capacity(n + m);
        ops.extend(old.iter().map(|l| (Op::Del, *l)));
        ops.extend(new.iter().map(|l| (Op::Ins, *l)));
        return ops;
    }

    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[idx(i, j)] = if old[i] == new[j] {
                dp[idx(i + 1, j + 1)] + 1
            } else {
                dp[idx(i + 1, j)].max(dp[idx(i, j + 1)])
            };
        }
    }

    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push((Op::Equal, old[i]));
            i += 1;
            j += 1;
        } else if dp[idx(i + 1, j)] >= dp[idx(i, j + 1)] {
            ops.push((Op::Del, old[i]));
            i += 1;
        } else {
            ops.push((Op::Ins, new[j]));
            j += 1;
        }
    }
    while i < n {
        ops.push((Op::Del, old[i]));
        i += 1;
    }
    while j < m {
        ops.push((Op::Ins, new[j]));
        j += 1;
    }
    ops
}

/// Render ops as a compact diff: changed lines (`-`/`+`) with `context` lines of
/// surrounding context; runs of unchanged lines outside the context window
/// collapse to a `⋮ (N unchanged lines)` marker.
fn render_hunks(ops: &[(Op, &str)], context: usize) -> String {
    let n = ops.len();
    let mut keep = vec![false; n];
    let mut any_change = false;
    for (i, (op, _)) in ops.iter().enumerate() {
        if *op != Op::Equal {
            any_change = true;
            let lo = i.saturating_sub(context);
            let hi = (i + context + 1).min(n);
            for slot in keep.iter_mut().take(hi).skip(lo) {
                *slot = true;
            }
        }
    }
    if !any_change {
        return String::new();
    }

    let mut out = String::new();
    let mut i = 0;
    while i < n {
        if !keep[i] {
            let start = i;
            while i < n && !keep[i] {
                i += 1;
            }
            let skipped = i - start;
            let plural = if skipped == 1 { "" } else { "s" };
            let _ = writeln!(out, "     ⋮ ({skipped} unchanged line{plural})");
            continue;
        }
        let (op, text) = ops[i];
        let prefix = match op {
            Op::Equal => " ",
            Op::Del => "-",
            Op::Ins => "+",
        };
        let _ = writeln!(out, "  {prefix} {text}");
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient, FakeLlmClient};
    use crate::test_support::{
        TestContext, agent_content, install_agent_on_disk, make_lock_entry_versioned,
        save_lock_with_entry, setup_source_with_agent, test_paths,
    };
    use crate::types::{ArtifactKind, InstallScope};
    use chrono::Utc;

    // --- line diff primitives ---

    #[test]
    fn lcs_ops_marks_inserts_and_deletes() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "x", "c"];
        let ops = lcs_ops(&old, &new);
        let added = ops.iter().filter(|(o, _)| *o == Op::Ins).count();
        let removed = ops.iter().filter(|(o, _)| *o == Op::Del).count();
        assert_eq!((added, removed), (1, 1), "one line replaced");
    }

    #[test]
    fn lcs_ops_identical_is_all_equal() {
        let v = vec!["a", "b"];
        let ops = lcs_ops(&v, &v);
        assert!(ops.iter().all(|(o, _)| *o == Op::Equal));
    }

    #[test]
    fn render_hunks_collapses_unchanged_runs() {
        // 10 equal lines, then one changed line.
        let mut old: Vec<&str> = (0..10).map(|_| "same").collect();
        old.push("old-tail");
        let mut new: Vec<&str> = (0..10).map(|_| "same").collect();
        new.push("new-tail");
        let ops = lcs_ops(&old, &new);
        let out = render_hunks(&ops, 3);
        assert!(out.contains("⋮"), "collapses the long unchanged run: {out}");
        assert!(out.contains("- old-tail"), "shows the removed line: {out}");
        assert!(out.contains("+ new-tail"), "shows the added line: {out}");
        assert!(!out.contains("⋮ (0 unchanged"), "no zero-length markers: {out}");
    }

    #[test]
    fn render_hunks_empty_for_no_changes() {
        let v = vec!["a", "b"];
        let ops = lcs_ops(&v, &v);
        assert!(render_hunks(&ops, 3).is_empty());
    }

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

    // --- reconciliations ---

    #[test]
    fn reconciliations_offers_promote_when_source_is_home() {
        let rs = reconciliations("pf", ArtifactKind::Skill, "home", true);
        assert_eq!(rs.len(), 2, "promote + update");
        assert!(rs[0].command.contains("promote pf"), "promote offered first: {:?}", rs[0]);
        assert!(rs[1].command.contains("update pf --force"), "update with force: {:?}", rs[1]);
        assert!(rs[1].note.is_some(), "force carries a caveat");
    }

    #[test]
    fn reconciliations_no_promote_for_git_source() {
        let rs = reconciliations("slidev", ArtifactKind::Skill, "guidelines", false);
        assert_eq!(rs.len(), 1, "only restore-from-source for a git source");
        assert!(rs[0].command.contains("update slidev"), "{:?}", rs[0]);
        assert!(!rs[0].command.contains("--force"), "no force when not locally modified");
        assert!(rs[0].description.contains("guidelines copy"), "names the source: {:?}", rs[0]);
    }

    // --- find_in_sources_with ---

    #[test]
    fn find_in_sources_locates_agent() {
        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let result = find_in_sources_with("my-agent", ArtifactKind::Agent, &ctx);
        assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    }

    #[test]
    fn find_in_sources_errors_when_not_found() {
        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "other-agent");

        let ctx = t.ctx();
        let result = find_in_sources_with("my-agent", ArtifactKind::Agent, &ctx);
        assert!(result.is_err());
    }

    // --- diff_with (top-level async) ---

    #[tokio::test]
    async fn diff_with_reports_up_to_date_when_checksums_match() {
        let t = TestContext::new();
        let content = agent_content("my-agent", "A test agent");
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        t.fs.add_file("/sources/my-source/agents/my-agent.md", content.clone());
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let output = diff("my-agent", ArtifactKind::Agent, false, &ctx).await.unwrap();
        assert!(output.is_up_to_date);
        assert!(output.reconciliations.is_empty(), "nothing to reconcile when in sync");
    }

    #[tokio::test]
    async fn diff_with_errors_without_llm_when_checksums_differ() {
        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            "different installed content",
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let result = diff("my-agent", ArtifactKind::Agent, false, &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("LLM"));
    }

    #[tokio::test]
    async fn gather_diff_populates_paths_changes_and_reconciliations() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("LLM analysis result");

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            "different installed content",
            InstallScope::Global,
        );
        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        let output = gather_diff_with("my-agent", ArtifactKind::Agent, &ctx).await.unwrap();

        assert!(!output.is_up_to_date);
        assert_eq!(output.analysis.as_deref(), Some("LLM analysis result"));
        assert!(!output.file_changes.is_empty(), "file change recorded");
        assert!(output.diff_text.is_some(), "unified diff present");
        assert!(output.installed_path.ends_with("my-agent.md"), "installed path set");
        assert!(!output.reconciliations.is_empty(), "reconciliation directions offered");
        assert!(output.installed_locally_edited, "edited after install (checksum mismatch)");
    }
}
