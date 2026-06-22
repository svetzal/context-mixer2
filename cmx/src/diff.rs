use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::fs_util;
use crate::lockfile;
use crate::platform::Platform;
use crate::source_iter;
use crate::types::{self, ArtifactKind, InstallScope};

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
    /// Every installed copy and how it compares to the source. With more than
    /// one entry the display shows a per-platform matrix; the detailed diff and
    /// analysis below focus the copy flagged `is_focus`.
    pub copies: Vec<CopyStatus>,
    /// Concrete name for the focused (changed) side — the platform whose copy is
    /// being shown, e.g. `codex`. Paired with `source_name` (e.g. `home`) these
    /// are the only two labels the output (and the LLM summary) uses, so the
    /// reader never has to map "installed"/"source" onto a real copy.
    pub changed_label: String,
}

/// One installed copy of the artifact and how it compares to the source.
#[derive(Debug, Clone)]
pub struct CopyStatus {
    /// The platforms whose install directory resolves to this copy (a shared
    /// `.agents/skills` copy lists several).
    pub platforms: Vec<Platform>,
    pub path: PathBuf,
    /// `true` when this copy is byte-identical to the source.
    pub matches: bool,
    pub added: usize,
    pub removed: usize,
    /// `true` for the copy whose detailed diff/analysis is shown below.
    pub is_focus: bool,
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
    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources_with(name, kind, ctx)?;
    let source_checksum = checksum::checksum_artifact(&source_path, kind, ctx.fs)?;

    // Discover every installed copy (skills can live on several platforms; a copy
    // matching source on one platform says nothing about the others).
    let (raw_copies, scope) = discover_copies(name, kind, ctx)?;
    if raw_copies.is_empty() {
        bail!("No installed {kind} named '{name}' found on disk.");
    }

    // Compare each copy to the source; build the per-copy diff for differing ones.
    let evals =
        evaluate_copies(raw_copies, kind, &source_checksum, &source_path, &source_name, ctx)?;

    // Focus the copy the user most likely means: the active platform's copy when
    // it differs, otherwise the first differing copy.
    let active = ctx.paths.platform;
    let focus = evals
        .iter()
        .position(|e| !e.matches && e.copy.platforms.contains(&active))
        .or_else(|| evals.iter().position(|e| !e.matches));

    let copies: Vec<CopyStatus> = evals
        .iter()
        .enumerate()
        .map(|(i, e)| CopyStatus {
            platforms: e.copy.platforms.clone(),
            path: e.copy.path.clone(),
            matches: e.matches,
            added: e.added,
            removed: e.removed,
            is_focus: Some(i) == focus,
        })
        .collect();

    // Every copy matches the source — nothing to reconcile anywhere.
    let Some(focus_idx) = focus else {
        return Ok(DiffOutput {
            artifact_name: name.to_string(),
            kind,
            is_up_to_date: true,
            installed_path: evals[0].copy.path.clone(),
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
            copies,
            changed_label: String::new(),
        });
    };

    let multi = copies.len() > 1;
    let focus_platform = representative_platform(&evals[focus_idx].copy, active, ctx);
    // The two labels the whole output uses: `home`/<repo> on the `−` side, the
    // platform name on the `+` side.
    let changed_label = focus_platform.to_string();

    // Version + "locally edited" come from the focus copy's lock baseline.
    let focus_checksum = evals[focus_idx].copy.checksum.clone();
    let (installed_version, locally_modified) =
        focus_lock_state(name, &evals[focus_idx].copy, &focus_checksum, scope, ctx)?;

    let reconciliations = reconciliations(
        name,
        kind,
        &source_name,
        &changed_label,
        locally_modified,
        multi.then_some(focus_platform),
    );

    let analysis = analyze_focus(
        kind,
        name,
        &source_name,
        &changed_label,
        source_version.as_deref().unwrap_or("unversioned"),
        installed_version.as_deref().unwrap_or("unversioned"),
        &evals[focus_idx].dir_diff.unified,
        ctx,
    )
    .await?;

    let focus_eval = &evals[focus_idx];
    Ok(DiffOutput {
        artifact_name: name.to_string(),
        kind,
        is_up_to_date: false,
        installed_path: focus_eval.copy.path.clone(),
        installed_version,
        installed_locally_edited: locally_modified,
        source_path,
        source_version,
        source_name,
        file_changes: focus_eval.dir_diff.changes.clone(),
        diff_text: Some(focus_eval.dir_diff.unified.clone()),
        analysis: Some(analysis),
        reconciliations,
        show_full: false,
        copies,
        changed_label,
    })
}

/// Ask the LLM to summarize the focused copy's diff, naming the two sides by
/// their concrete identities (`source_name`, `changed`) so the summary speaks the
/// same language as the rest of the output (never "source"/"installed").
#[allow(clippy::too_many_arguments)]
async fn analyze_focus(
    kind: ArtifactKind,
    name: &str,
    source_name: &str,
    changed: &str,
    source_ver: &str,
    changed_ver: &str,
    unified: &str,
    ctx: &AppContext<'_>,
) -> Result<String> {
    let system_prompt = format!(
        "You are a technical analyst comparing two copies of an AI coding assistant {kind} \
        (written in markdown). You are given a unified diff: lines prefixed with `-` belong to \
        the '{source_name}' copy; lines prefixed with `+` belong to the '{changed}' copy. \
        Refer to the two copies as '{source_name}' and '{changed}' — do not call them \
        \"source\" or \"installed\". Provide a clear, concise summary. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed (and in which copy)\n\
        2. Whether the difference is significant or cosmetic\n\
        3. A recommendation: which copy looks more authoritative, and which way to reconcile\n\n\
        Keep it brief and actionable — a few paragraphs at most."
    );
    let user_prompt = format!(
        "Compare these two copies of the {kind} '{name}':\n\
        - '{source_name}' copy (the `−` lines): {source_ver}\n\
        - '{changed}' copy (the `+` lines): {changed_ver}\n\n\
        {unified}"
    );
    match ctx.llm {
        Some(llm) => llm.analyze(&system_prompt, &user_prompt).await,
        None => bail!("LLM client not configured for diff analysis"),
    }
}

/// One installed copy with its computed comparison to the source.
struct CopyEval {
    copy: InstalledCopy,
    matches: bool,
    dir_diff: ArtifactDiff,
    added: usize,
    removed: usize,
}

/// Compare each discovered copy to the source, computing the per-copy diff (and
/// its +/- totals) for the ones that differ.
fn evaluate_copies(
    raw_copies: Vec<InstalledCopy>,
    kind: ArtifactKind,
    source_checksum: &str,
    source_path: &Path,
    source_name: &str,
    ctx: &AppContext<'_>,
) -> Result<Vec<CopyEval>> {
    let mut evals = Vec::with_capacity(raw_copies.len());
    for copy in raw_copies {
        let matches = copy.checksum == source_checksum;
        let dir_diff = if matches {
            ArtifactDiff {
                changes: Vec::new(),
                unified: String::new(),
            }
        } else {
            diff_artifact(kind, &copy.path, source_path, source_name, ctx)?
        };
        let added = dir_diff.changes.iter().map(|c| c.added).sum();
        let removed = dir_diff.changes.iter().map(|c| c.removed).sum();
        evals.push(CopyEval {
            copy,
            matches,
            dir_diff,
            added,
            removed,
        });
    }
    Ok(evals)
}

/// A distinct physical install of the artifact, shared by ≥1 platform.
struct InstalledCopy {
    platforms: Vec<Platform>,
    path: PathBuf,
    checksum: String,
}

/// Discover every installed copy of the artifact and the scope it lives at.
///
/// Skills can be installed on several platforms (some sharing the
/// `.agents/skills` directory), so they're surveyed across the managed
/// platforms. Agents are reformatted per platform (e.g. Codex TOML), so a
/// cross-platform byte comparison is meaningless — they stay single-copy on the
/// active platform.
fn discover_copies(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(Vec<InstalledCopy>, InstallScope)> {
    if kind == ArtifactKind::Agent {
        return match config::find_installed_path(name, kind, ctx.fs, ctx.paths) {
            Some((path, scope)) => {
                let checksum = checksum::checksum_artifact(&path, kind, ctx.fs)?;
                Ok((
                    vec![InstalledCopy {
                        platforms: vec![ctx.paths.platform],
                        path,
                        checksum,
                    }],
                    scope,
                ))
            }
            None => Ok((Vec::new(), InstallScope::Global)),
        };
    }
    // Skills: global scope first, then project.
    for scope in InstallScope::ALL {
        let copies = gather_skill_copies(name, scope, ctx)?;
        if !copies.is_empty() {
            return Ok((copies, scope));
        }
    }
    Ok((Vec::new(), InstallScope::Global))
}

/// Gather distinct skill copies across the managed platforms at `scope`, one
/// entry per install directory (the shared `.agents/skills` dir collapses
/// several platforms into one copy).
fn gather_skill_copies(
    name: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<InstalledCopy>> {
    let candidates =
        config::managed_platforms(ctx.fs, ctx.paths)?.unwrap_or_else(|| Platform::ALL.to_vec());
    let mut by_dir: BTreeMap<PathBuf, InstalledCopy> = BTreeMap::new();
    for platform in candidates {
        if !platform.supports(ArtifactKind::Skill) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        let Some(path) = pv.installed_artifact_path(ArtifactKind::Skill, name, scope) else {
            continue;
        };
        if !ctx.fs.exists(&path) {
            continue;
        }
        if let Some(existing) = by_dir.get_mut(&path) {
            existing.platforms.push(platform);
        } else {
            let checksum = checksum::checksum_artifact(&path, ArtifactKind::Skill, ctx.fs)?;
            by_dir.insert(
                path.clone(),
                InstalledCopy {
                    platforms: vec![platform],
                    path,
                    checksum,
                },
            );
        }
    }
    Ok(by_dir.into_values().collect())
}

/// Pick the platform to name in reconcile commands for a copy shared by several:
/// the active platform if it reads this copy, else a managed platform, else the
/// first — so `--platform codex` is suggested over `--platform opencode`.
fn representative_platform(
    copy: &InstalledCopy,
    active: Platform,
    ctx: &AppContext<'_>,
) -> Platform {
    if copy.platforms.contains(&active) {
        return active;
    }
    let managed = config::managed_platforms(ctx.fs, ctx.paths).ok().flatten();
    managed
        .as_ref()
        .and_then(|m| copy.platforms.iter().find(|p| m.contains(p)).copied())
        .or_else(|| copy.platforms.first().copied())
        .unwrap_or(active)
}

/// Read the focus copy's lock baseline (from any platform that reads it): its
/// recorded version, and whether the copy was edited after install (its bytes no
/// longer match the lock's checksum).
fn focus_lock_state(
    name: &str,
    copy: &InstalledCopy,
    checksum: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<(Option<String>, bool)> {
    for &platform in &copy.platforms {
        let pv = ctx.paths.with_platform(platform);
        if let Some(entry) = lockfile::load(scope, ctx.fs, &pv)?.packages.get(name) {
            return Ok((entry.version.clone(), entry.installed_checksum != checksum));
        }
    }
    Ok((None, false))
}

/// Build the reconciliation directions, naming the two copies concretely
/// (`{changed}` is the edited platform copy, `{source_name}` the canonical one).
/// When the source is the home, `{changed}`'s edits can be promoted into it;
/// either way they can be discarded by re-installing. `diff` never picks for the
/// user. `platform`, set when copies span platforms, qualifies the commands.
fn reconciliations(
    name: &str,
    kind: ArtifactKind,
    source_name: &str,
    changed: &str,
    locally_modified: bool,
    platform: Option<Platform>,
) -> Vec<Reconciliation> {
    let mut out = Vec::new();
    let source_is_home = source_name == crate::adopt::HOME_SOURCE;
    let plat = platform.map(|p| format!(" --platform {p}")).unwrap_or_default();

    if source_is_home {
        out.push(Reconciliation {
            description: format!("keep {changed}'s edits — copy {changed} into {source_name}"),
            command: format!("cmx {kind} promote {name}{plat}"),
            note: None,
        });
    }

    out.push(Reconciliation {
        description: format!("discard {changed}'s edits — restore {changed} from {source_name}"),
        command: if locally_modified {
            format!("cmx {kind} update {name}{plat} --force")
        } else {
            format!("cmx {kind} update {name}{plat}")
        },
        note: locally_modified.then(|| format!("--force overwrites {changed}'s local edits")),
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
        let rs = reconciliations("pf", ArtifactKind::Skill, "home", "claude", true, None);
        assert_eq!(rs.len(), 2, "promote + update");
        assert!(rs[0].command.contains("promote pf"), "promote offered first: {:?}", rs[0]);
        assert!(rs[1].command.contains("update pf --force"), "update with force: {:?}", rs[1]);
        // Descriptions name the concrete copies, not "installed"/"source".
        assert!(
            rs[0].description.contains("claude") && rs[0].description.contains("home"),
            "{:?}",
            rs[0]
        );
        assert!(!rs[0].description.contains("installed"), "no abstract 'installed': {:?}", rs[0]);
        assert!(
            rs[1].note.as_deref().unwrap().contains("claude"),
            "caveat names the copy: {:?}",
            rs[1]
        );
    }

    #[test]
    fn reconciliations_no_promote_for_git_source() {
        let rs = reconciliations("slidev", ArtifactKind::Skill, "guidelines", "codex", false, None);
        assert_eq!(rs.len(), 1, "only restore-from-source for a git source");
        assert!(rs[0].command.contains("update slidev"), "{:?}", rs[0]);
        assert!(!rs[0].command.contains("--force"), "no force when not locally modified");
        assert!(rs[0].description.contains("guidelines"), "names the source: {:?}", rs[0]);
        assert!(rs[0].description.contains("codex"), "names the changed copy: {:?}", rs[0]);
    }

    #[test]
    fn reconciliations_qualify_commands_with_platform_when_multi() {
        let rs = reconciliations(
            "pf",
            ArtifactKind::Skill,
            "home",
            "codex",
            true,
            Some(Platform::Codex),
        );
        assert!(rs[0].command.contains("promote pf --platform codex"), "{:?}", rs[0]);
        assert!(rs[1].command.contains("update pf --platform codex --force"), "{:?}", rs[1]);
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

    #[tokio::test]
    async fn gather_diff_skill_focuses_the_differing_platform() {
        use crate::test_support::{setup_source, skill_content};

        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("analysis");

        // Source is the home; the Claude copy matches it, the Codex copy differs.
        let source = skill_content("the canonical skill");
        setup_source(&fs, &paths, "home", "/home-src");
        fs.add_file("/home-src/pf/SKILL.md", source.clone());
        let claude = paths.with_platform(crate::platform::Platform::Claude);
        fs.add_file(
            claude
                .install_dir(ArtifactKind::Skill, InstallScope::Global)
                .unwrap()
                .join("pf/SKILL.md"),
            source,
        );
        let codex = paths.with_platform(crate::platform::Platform::Codex);
        fs.add_file(
            codex
                .install_dir(ArtifactKind::Skill, InstallScope::Global)
                .unwrap()
                .join("pf/SKILL.md"),
            skill_content("the codex edits"),
        );
        // Scope the survey + suggestions to the two managed platforms.
        let config = crate::types::CmxConfig {
            platforms: vec![
                crate::platform::Platform::Claude,
                crate::platform::Platform::Codex,
            ],
            ..Default::default()
        };
        crate::config::save_config(&config, &fs, &paths).unwrap();

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        // Active platform is Claude (the matching copy), yet diff must surface the
        // Codex divergence rather than report "matches".
        let output = gather_diff_with("pf", ArtifactKind::Skill, &ctx).await.unwrap();

        assert!(!output.is_up_to_date, "must not claim up-to-date while a copy differs");
        assert_eq!(output.copies.len(), 2, "both platform copies surveyed");
        let focus = output.copies.iter().find(|c| c.is_focus).expect("a focus copy");
        assert!(focus.platforms.contains(&crate::platform::Platform::Codex), "focuses Codex");
        assert!(!focus.matches, "the focused copy differs");
        assert!(
            output
                .copies
                .iter()
                .any(|c| c.platforms.contains(&crate::platform::Platform::Claude) && c.matches),
            "the Claude copy is reported as matching"
        );
        assert!(
            output.reconciliations[0].command.contains("--platform codex"),
            "reconcile qualified to the diverging platform: {:?}",
            output.reconciliations[0]
        );
    }
}
