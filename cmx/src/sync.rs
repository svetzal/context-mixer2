//! `cmx skill sync` — reconcile a skill that has **diverged across platforms**.
//!
//! Unlike [`crate::install::update`], which re-installs from a registered
//! *source*, `sync` reconciles **between install locations**: it picks one
//! copy as the winner and overwrites the others so every platform carries the
//! same content. That makes it the only reconciliation that works for skills
//! with no source — including `external` ones (e.g. a skill another tool
//! installs into several places at different versions).
//!
//! Skills only. Agents are reformatted per platform (e.g. Codex TOML), so a
//! cross-platform agent copy is not a byte-for-byte operation and is rejected
//! at the command layer.

use anyhow::{Result, bail};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::adopt::HOME_SOURCE;
use crate::checksum;
use crate::context::AppContext;
use crate::copy;
use crate::diff::{FileChange, file_changes_between};
use crate::lockfile;
use crate::platform::{Platform, platforms_label};
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// One install location to be brought into line with the winner.
#[derive(Debug)]
pub struct SyncTarget {
    /// Platforms whose install directory resolves to this location.
    pub platforms: Vec<Platform>,
    pub location: PathBuf,
    pub artifact_path: PathBuf,
    /// The version this copy carried before the sync.
    pub from_version: Option<String>,
    /// Per-file changes this target will receive (or received).
    pub file_changes: Vec<FileChange>,
}

#[derive(Debug)]
pub struct SyncResult {
    pub name: String,
    pub apply: bool,
    /// `true` when the skill matched an `external` rule — reconciled anyway,
    /// but the user is told another tool may re-diverge it.
    pub external: bool,
    /// Platforms that provided the winning copy.
    pub winner_platforms: Vec<Platform>,
    pub winner_path: PathBuf,
    pub winner_version: Option<String>,
    /// `true` when every copy already matched — nothing to do.
    pub already_synced: bool,
    /// Locations changed (or, in plan mode, that would change).
    pub targets: Vec<SyncTarget>,
}

// ---------------------------------------------------------------------------
// One physical copy of the skill (shared by ≥1 platform)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Copy {
    /// Every candidate platform whose install dir resolves to this location.
    platforms: Vec<Platform>,
    /// The install directory (the copy lives at `dir/<name>`).
    dir: PathBuf,
    /// The installed skill path (its directory).
    path: PathBuf,
    version: Option<String>,
    checksum: String,
}

impl Copy {
    fn as_target(&self, winner: &Copy, ctx: &AppContext<'_>) -> Result<SyncTarget> {
        Ok(SyncTarget {
            platforms: self.platforms.clone(),
            location: self.dir.clone(),
            artifact_path: self.path.clone(),
            from_version: self.version.clone(),
            file_changes: file_changes_between(ArtifactKind::Skill, &self.path, &winner.path, ctx)?,
        })
    }
}

/// Whether any copy of `name` matches an `external` rule (managed by another
/// tool). `sync` reconciles it anyway but tells the user.
fn is_external(name: &str, copies: &[Copy], ctx: &AppContext<'_>) -> Result<bool> {
    let rules = crate::config::load_config(ctx.fs, ctx.paths)?.external;
    Ok(copies
        .iter()
        .any(|c| crate::config::matches_external(&rules, name, &c.dir, &ctx.paths.home_dir)))
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Compare two version strings "newest-first": numeric dotted segments compared
/// numerically, with a non-numeric segment falling back to lexical order. An
/// absent version sorts below any present one.
fn cmp_versions(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => {
            let mut ai = a.split('.');
            let mut bi = b.split('.');
            loop {
                match (ai.next(), bi.next()) {
                    (None, None) => return Ordering::Equal,
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (Some(x), Some(y)) => {
                        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                            (Ok(xn), Ok(yn)) => xn.cmp(&yn),
                            _ => x.cmp(y),
                        };
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                }
            }
        }
    }
}

/// The copy a `--from <platform>` selection points at, or an error if that
/// platform has no copy.
fn pick_from(copies: &[Copy], p: Platform) -> Result<&Copy> {
    copies
        .iter()
        .find(|c| c.platforms.contains(&p))
        .ok_or_else(|| anyhow::anyhow!("'{p}' has no copy of this skill to sync from."))
}

/// The copy with the strictly-newest version, or `None` when the choice is
/// ambiguous: another *differing* copy ties it on version, or all are
/// unversioned, or the slice is empty. The caller turns `None` into an
/// actionable error.
fn auto_winner(copies: &[Copy]) -> Option<&Copy> {
    let best = copies
        .iter()
        .max_by(|x, y| cmp_versions(x.version.as_deref(), y.version.as_deref()))?;
    let ambiguous = copies.iter().any(|c| {
        c.checksum != best.checksum
            && cmp_versions(c.version.as_deref(), best.version.as_deref()) == Ordering::Equal
    });
    (!ambiguous).then_some(best)
}

/// Render a byte count compactly (e.g. `4.2 KB`), using integer math to avoid a
/// lossy float cast.
fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{}.{} KB", bytes / 1024, (bytes % 1024) * 10 / 1024)
    }
}

/// Whether any copy of `name` is tracked from the canonical `home` source — if
/// so, the user can also promote a copy and re-project instead of picking
/// between install locations.
fn is_home_tracked(
    name: &str,
    scope: InstallScope,
    copies: &[Copy],
    ctx: &AppContext<'_>,
) -> Result<bool> {
    for c in copies {
        for &platform in &c.platforms {
            let pv = ctx.paths.with_platform(platform);
            let tracked_from_home = lockfile::load(scope, ctx.fs, &pv)?
                .packages
                .get(name)
                .is_some_and(|e| e.source.repo == HOME_SOURCE);
            if tracked_from_home {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Build the actionable error for an ambiguous auto-pick: list each diverging
/// copy (platforms, location, size), the exact `--from` command per copy, and —
/// when the skill is home-tracked — the `promote` alternative.
fn ambiguity_error(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    copies: &[Copy],
    managed: Option<&[Platform]>,
    ctx: &AppContext<'_>,
) -> anyhow::Error {
    let mut msg = format!(
        "Can't tell which copy of '{name}' is newest — the differing copies are unversioned \
         or share a version.\n"
    );
    for c in copies {
        let size = ctx.fs.read_to_string(&kind.content_path(&c.path)).map_or(0, |s| s.len());
        let _ = writeln!(
            msg,
            "  {}  {}  ({})",
            platforms_label(&c.platforms),
            c.dir.display(),
            human_size(size)
        );
    }
    // Prefer a managed platform when naming the `--from` for a copy shared by
    // several platforms (the `.agents/skills` cohort), so the suggestion reads
    // in terms of a tool the user actually uses (e.g. `codex`, not `opencode`).
    let representative = |c: &Copy| -> Option<Platform> {
        managed
            .and_then(|m| c.platforms.iter().find(|p| m.contains(p)).copied())
            .or_else(|| c.platforms.first().copied())
    };
    let _ = writeln!(msg, "Choose which copy wins:");
    for c in copies {
        if let Some(p) = representative(c) {
            let _ = writeln!(msg, "  cmx {kind} sync {name} --from {p}");
        }
    }
    if is_home_tracked(name, scope, copies, ctx).unwrap_or(false) {
        let _ = write!(
            msg,
            "This skill is tracked from the home — you can also make one copy canonical and \
             re-project it:\n  cmx {kind} promote {name}"
        );
    }
    anyhow::anyhow!(msg)
}

/// Gather the distinct physical copies of a skill across the candidate
/// platforms, one entry per install directory (the shared `.agents/skills` dir
/// collapses several platforms into one location).
fn gather_copies(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<Copy>> {
    let candidates = crate::config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    let mut by_dir: BTreeMap<PathBuf, Copy> = BTreeMap::new();
    for platform in candidates {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        let Some(path) = pv.installed_artifact_path(kind, name, scope) else {
            continue;
        };
        if !ctx.fs.exists(&path) {
            continue;
        }
        let dir = pv.require_install_dir(kind, scope)?;
        if let Some(existing) = by_dir.get_mut(&dir) {
            existing.platforms.push(platform);
        } else {
            let content = ctx.fs.read_to_string(&kind.content_path(&path)).ok();
            let version = content.as_deref().and_then(crate::scan::extract_version_from_content);
            let checksum = checksum::checksum_artifact(&path, kind, ctx.fs)?;
            by_dir.insert(
                dir.clone(),
                Copy {
                    platforms: vec![platform],
                    dir,
                    path,
                    version,
                    checksum,
                },
            );
        }
    }
    Ok(by_dir.into_values().collect())
}

/// Overwrite each diverging copy with the winner's content, then refresh every
/// tracked lock entry (winner + targets) so they agree on checksum and version.
/// Copies with no lock entry (external) are left untracked — only their files
/// are equalized.
fn apply_winner(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    winner: &Copy,
    targets: &[&Copy],
    ctx: &AppContext<'_>,
) -> Result<()> {
    for target in targets {
        crate::uninstall::remove_installed(kind, &target.path, ctx.fs)?;
        copy::copy_artifact(&winner.path, &target.dir, kind, name, ctx)?;
    }
    let winner_checksum = checksum::checksum_artifact(&winner.path, kind, ctx.fs)?;
    let now = ctx.clock.now().to_rfc3339();
    for copy_ in targets.iter().chain(std::iter::once(&winner)) {
        for &platform in &copy_.platforms {
            let pv = ctx.paths.with_platform(platform);
            if lockfile::load(scope, ctx.fs, &pv)?.packages.contains_key(name) {
                lockfile::mutate(scope, ctx.fs, &pv, |lock| {
                    if let Some(entry) = lock.packages.get_mut(name) {
                        entry.installed_checksum.clone_from(&winner_checksum);
                        entry.version.clone_from(&winner.version);
                        entry.installed_at.clone_from(&now);
                    }
                })?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Reconcile a skill across the managed platforms by copying the winning copy
/// over the others. `from` forces the winner; otherwise the newest version wins.
pub fn sync(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    from: Option<Platform>,
    apply: bool,
    ctx: &AppContext<'_>,
) -> Result<SyncResult> {
    if kind != ArtifactKind::Skill {
        bail!(
            "`sync` currently supports skills only. Agents are reformatted per platform \
             (e.g. Codex TOML), so cross-platform agent reconciliation needs format-aware \
             handling (not yet implemented)."
        );
    }

    let copies = gather_copies(name, kind, scope, ctx)?;
    if copies.is_empty() {
        bail!(
            "Skill '{name}' is not installed on any managed platform. {}",
            crate::suggestions::installed_artifact_hint(name, Some(ArtifactKind::Skill), ctx)
        );
    }

    let external = is_external(name, &copies, ctx)?;

    // Already consistent when every copy shares one checksum (covers the
    // single-copy case too).
    if copies.iter().all(|c| c.checksum == copies[0].checksum) {
        return Ok(SyncResult {
            name: name.to_string(),
            apply,
            external,
            winner_platforms: copies[0].platforms.clone(),
            winner_path: copies[0].path.clone(),
            winner_version: copies[0].version.clone(),
            already_synced: true,
            targets: Vec::new(),
        });
    }

    let winner = if let Some(p) = from {
        pick_from(&copies, p)?
    } else if let Some(w) = auto_winner(&copies) {
        w
    } else {
        // Load the managed set here (propagating I/O errors) rather than
        // swallowing them inside the error constructor.
        let managed = crate::config::managed_platforms(ctx.fs, ctx.paths)?;
        return Err(ambiguity_error(name, kind, scope, &copies, managed.as_deref(), ctx));
    };
    let targets: Vec<&Copy> = copies.iter().filter(|c| c.checksum != winner.checksum).collect();

    let result = SyncResult {
        name: name.to_string(),
        apply,
        external,
        winner_platforms: winner.platforms.clone(),
        winner_path: winner.path.clone(),
        winner_version: winner.version.clone(),
        already_synced: false,
        targets: targets.iter().map(|c| c.as_target(winner, ctx)).collect::<Result<_>>()?,
    };

    if apply {
        apply_winner(name, kind, scope, winner, &targets, ctx)?;
    }
    Ok(result)
}

#[cfg(test)]
#[path = "sync/tests.rs"]
mod tests;
