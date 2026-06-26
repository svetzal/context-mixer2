use anyhow::Result;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::platform::Platform;
use crate::types::{ArtifactKind, InstallScope};

use super::structural::ArtifactDiff;

/// A distinct physical install of the artifact, shared by ≥1 platform.
pub(super) struct InstalledCopy {
    pub(super) platforms: Vec<Platform>,
    pub(super) path: PathBuf,
    pub(super) checksum: String,
}

/// One installed copy with its computed comparison to the source.
pub(super) struct CopyEval {
    pub(super) copy: InstalledCopy,
    pub(super) matches: bool,
    pub(super) dir_diff: ArtifactDiff,
    pub(super) added: usize,
    pub(super) removed: usize,
}

/// Discover every installed copy of the artifact and the scope it lives at.
///
/// Skills can be installed on several platforms (some sharing the
/// `.agents/skills` directory), so they're surveyed across the managed
/// platforms. Agents are reformatted per platform (e.g. Codex TOML), so a
/// cross-platform byte comparison is meaningless — they stay single-copy on the
/// active platform.
pub(super) fn discover_copies(
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
    let candidates = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
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
pub(super) fn representative_platform(
    copy: &InstalledCopy,
    active: Platform,
    managed: Option<&[Platform]>,
) -> Platform {
    if copy.platforms.contains(&active) {
        return active;
    }
    managed
        .and_then(|m| copy.platforms.iter().find(|p| m.contains(p)).copied())
        .or_else(|| copy.platforms.first().copied())
        .unwrap_or(active)
}

/// Compare each discovered copy to the source, computing the per-copy diff (and
/// its +/- totals) for the ones that differ.
pub(super) fn evaluate_copies(
    raw_copies: Vec<InstalledCopy>,
    kind: ArtifactKind,
    source_checksum: &str,
    source_path: &std::path::Path,
    source_name: &str,
    ctx: &AppContext<'_>,
) -> Result<Vec<CopyEval>> {
    use super::structural::diff_artifact;
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
