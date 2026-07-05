use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use crate::artifact_status;
use crate::checksum;
use crate::context::AppContext;
use crate::copy;
use crate::diff::{FileChange, FileStatus, file_changes_between};
use crate::lockfile;
use crate::paths::ConfigPaths;
use crate::platform::Platform;
use crate::source_iter;
use crate::types::{self, ArtifactKind, InstallScope, LockEntry, LockSource};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct InstallResult {
    pub artifact_name: String,
    pub version: Option<String>,
    pub kind: ArtifactKind,
    pub source_name: String,
    pub dest_dir: PathBuf,
    /// The platform this copy was installed for.
    pub platform: Platform,
    /// Concrete target files whose local changes were discarded by `--force`.
    pub discarded_paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct BatchInstallResult {
    pub items: Vec<InstallResult>,
    pub kind: ArtifactKind,
    pub is_update: bool,
}

/// Outcome of installing several named artifacts in one pass.
#[derive(Debug)]
pub struct InstallManyResult {
    pub kind: ArtifactKind,
    pub installed: Vec<InstallResult>,
    /// `(name, reason)` for names that failed (not found, ambiguous, locally
    /// modified without `--force`, …).
    pub failed: Vec<(String, String)>,
}

/// Pure description of an intended installation — computed from source metadata
/// and path configuration, with no filesystem access.
#[derive(Debug)]
pub struct InstallPlan {
    pub artifact_name: String,
    pub version: Option<String>,
    pub source_name: String,
    pub source_root: PathBuf,
    pub dest_dir: PathBuf,
    pub relative_path: String,
}

pub fn install(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    ctx.paths.ensure_supports(kind)?;

    let (source_name, artifact_name) = parse_name(name);
    let found = source_iter::find_unique(artifact_name, kind, source_name, ctx)?;
    let plan = plan_install(artifact_name, kind, scope, &found, ctx.paths)?;
    ctx.fs.create_dir_all(&plan.dest_dir)?;
    let source_checksum = checksum::checksum_artifact(&found.artifact.path, kind, ctx.fs)?;

    let facts = gather_install_facts(artifact_name, kind, scope, force, ctx)?;
    let decision = decide_install(facts.already_installed, facts.locally_modified, force);
    if decision.blocked {
        bail!(
            "'{artifact_name}' has local modifications. Use --force to overwrite, \
             or 'cmx {kind} diff {artifact_name}' to review changes first."
        );
    }
    let discarded_paths = if force && facts.locally_modified {
        collect_discarded_paths(
            kind,
            &ctx.paths.require_installed_artifact_path(kind, artifact_name, scope)?,
            &found.artifact.path,
            ctx,
        )?
    } else {
        Vec::new()
    };

    commit_install(&plan, kind, scope, &found.artifact.path, source_checksum, &decision, ctx)?;

    Ok(InstallResult {
        artifact_name: artifact_name.to_string(),
        version: plan.version,
        kind,
        source_name: plan.source_name,
        dest_dir: plan.dest_dir,
        platform: ctx.paths.platform,
        discarded_paths,
    })
}

/// Resolve the platforms an install targets, given the optional `--platform`
/// selector.
///
/// - `Some(p)` — exactly that platform (a later `ensure_supports` check fails
///   loudly if it can't host `kind`).
/// - `None`, explicit managed set configured — every managed platform that
///   supports `kind`. The user has declared which tools cmx manages, so that
///   list is authoritative.
/// - `None`, no managed set — every platform already **in use** (a non-empty
///   lock file at `scope`) that supports `kind`, so a default install lands in
///   the tools you actually use rather than scattering into all supported
///   tools. Falls back to `[Claude]` when nothing is tracked yet, so a
///   first-ever install still has a sensible home.
pub fn resolve_targets(
    selector: Option<Platform>,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    crate::targets::resolve_targets(selector, kind, scope, ctx)
}

/// Install several named artifacts in one pass, into each of `targets`.
///
/// Best-effort along two axes: each name is installed independently, and each
/// target platform is attempted independently. A name yields one
/// [`InstallResult`] per platform it lands on. A name is only recorded as
/// `failed` when it fails on **every** target (e.g. not found in any source,
/// ambiguous) — a per-platform failure (such as a locally-modified copy on one
/// tool) doesn't discard the successes on the others. Backs
/// `cmx {skill,agent} install <name>...`.
pub fn install_many(
    names: &[String],
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    targets: &[Platform],
    ctx: &AppContext<'_>,
) -> Result<InstallManyResult> {
    let mut installed = Vec::new();
    let mut failed = Vec::new();
    for name in names {
        let mut landed = false;
        let mut last_err: Option<String> = None;
        for &platform in targets {
            let pv = ctx.paths.with_platform(platform);
            let pctx = ctx.with_paths(&pv);
            match install(name, kind, scope, force, &pctx) {
                Ok(r) => {
                    landed = true;
                    installed.push(r);
                }
                Err(e) => last_err = Some(e.to_string()),
            }
        }
        if !landed {
            failed.push((
                name.clone(),
                last_err.unwrap_or_else(|| "no target platforms".to_string()),
            ));
        }
    }
    Ok(InstallManyResult {
        kind,
        installed,
        failed,
    })
}

pub fn update(
    name: &str,
    kind: ArtifactKind,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    let Some((entry, scope)) = lockfile::find_entry(name, ctx.fs, ctx.paths)? else {
        bail!(
            "No installed {kind} named '{name}' found. {}",
            crate::suggestions::installed_artifact_hint(name, Some(kind), ctx)
        );
    };
    let pinned = format!("{}:{}", entry.source.repo, name);
    install(&pinned, kind, scope, force, ctx)
}

/// Install every available artifact of `kind` from the sources into each of
/// `targets`, concatenating the per-platform results.
pub fn install_all(
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    targets: &[Platform],
    ctx: &AppContext<'_>,
) -> Result<BatchInstallResult> {
    let mut items = Vec::new();
    for &platform in targets {
        let pv = ctx.paths.with_platform(platform);
        let pctx = ctx.with_paths(&pv);
        items.extend(install_all_one(kind, scope, force, &pctx)?.items);
    }
    Ok(BatchInstallResult {
        items,
        kind,
        is_update: false,
    })
}

fn install_all_one(
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<BatchInstallResult> {
    ctx.paths.ensure_supports(kind)?;

    let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
    let mut installed = Vec::new();

    for sa in source_iter::all_artifacts(ctx)? {
        if sa.artifact.kind != kind {
            continue;
        }
        // Skip if the source is not considered outdated relative to the lock entry
        if let Some(lock_entry) = lock.packages.get(&sa.artifact.name) {
            let source_cs = checksum::checksum_artifact(&sa.artifact.path, kind, ctx.fs)?;
            if !artifact_status::source_outdated(
                Some(lock_entry),
                &source_cs,
                sa.artifact.version.as_deref(),
            ) {
                continue;
            }
        }
        let pinned = format!("{}:{}", sa.source_name, sa.artifact.name);
        let result = install(&pinned, kind, scope, force, ctx)?;
        installed.push(result);
    }

    Ok(BatchInstallResult {
        items: installed,
        kind,
        is_update: false,
    })
}

pub fn update_all(
    kind: ArtifactKind,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<BatchInstallResult> {
    ctx.paths.ensure_supports(kind)?;

    let all_source_info = source_iter::all_with_checksums(ctx)?;
    let mut updated = Vec::new();

    let locks = lockfile::load_both(ctx.fs, ctx.paths)?;
    for (scope, lock) in &locks {
        for (name, entry) in &lock.packages {
            if entry.artifact_type != kind {
                continue;
            }

            if let Some(source_infos) = all_source_info.get(name)
                && source_infos.iter().any(|si| {
                    si.source_name == entry.source.repo
                        && artifact_status::source_outdated(
                            Some(entry),
                            &si.checksum,
                            si.version.as_deref(),
                        )
                })
            {
                let pinned = format!("{}:{name}", entry.source.repo);
                let result = install(&pinned, kind, *scope, force, ctx)?;
                updated.push(result);
            }
        }
    }

    Ok(BatchInstallResult {
        items: updated,
        kind,
        is_update: true,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// I/O facts gathered before the pure install decision is made.
///
/// `pub(crate)` so `cmx set activate`/`deactivate` (see `sets.rs`) can reuse the
/// same drift/already-installed detection install itself uses, rather than
/// reimplementing it.
pub(crate) struct InstallFacts {
    pub(crate) locally_modified: bool,
    pub(crate) already_installed: bool,
}

/// Gather the I/O facts needed to decide whether an install should proceed.
/// All filesystem access for the decision lives here; the caller passes the
/// result to the pure [`decide_install`].
pub(crate) fn gather_install_facts(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallFacts> {
    let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
    let locally_modified = check_local_modifications(
        artifact_name,
        kind,
        scope,
        lock.packages.get(artifact_name),
        ctx,
    )?;
    let already_installed = ctx.paths.is_installed(kind, artifact_name, scope, ctx.fs);
    Ok(InstallFacts {
        locally_modified: locally_modified && (!force || already_installed),
        already_installed,
    })
}

/// Copy the artifact, checksum the installed copy, write the lock entry, and
/// roll back the copy if the lockfile write fails (fresh installs only).
fn commit_install(
    plan: &InstallPlan,
    kind: ArtifactKind,
    scope: InstallScope,
    source_path: &std::path::Path,
    source_checksum: String,
    decision: &InstallDecision,
    ctx: &AppContext<'_>,
) -> Result<PathBuf> {
    if decision.replace_existing {
        let existing =
            kind.installed_path(&plan.artifact_name, &plan.dest_dir, ArtifactKind::HOME_AGENT_EXT);
        if ctx.fs.exists(&existing) {
            crate::uninstall::remove_installed(kind, &existing, ctx.fs)?;
        }
    }
    let dest_path =
        copy::copy_artifact(source_path, &plan.dest_dir, kind, &plan.artifact_name, ctx)?;
    let installed_checksum = checksum::checksum_artifact(&dest_path, kind, ctx.fs)?;

    let lock_result = lockfile::mutate(scope, ctx.fs, ctx.paths, |lock| {
        lock.packages.insert(
            plan.artifact_name.clone(),
            build_lock_entry(
                plan,
                kind,
                source_checksum,
                installed_checksum,
                ctx.clock.now().to_rfc3339(),
            ),
        );
    });

    if let Err(lock_err) = lock_result {
        // If we performed a fresh install and the lockfile write failed, roll
        // back by removing the artifact we just copied.  This avoids leaving a
        // ghost: an artifact on disk with no lockfile entry.  We ignore any
        // remove error to ensure the original lock error is surfaced.
        if decision.rollback_on_lock_fail {
            let _ = crate::uninstall::remove_installed(kind, &dest_path, ctx.fs);
        }
        return Err(lock_err);
    }

    Ok(dest_path)
}

/// Compute the destination directory and relative source path for an install.
/// Pure function — no filesystem access.
pub(crate) fn plan_install(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    found: &source_iter::SourceArtifact,
    paths: &ConfigPaths,
) -> Result<InstallPlan> {
    let dest_dir = paths.require_install_dir(kind, scope)?;
    let relative_path = types::relative_path_string(&found.artifact.path, &found.source_root);
    Ok(InstallPlan {
        artifact_name: artifact_name.to_string(),
        version: found.artifact.version.clone(),
        source_name: found.source_name.clone(),
        source_root: found.source_root.clone(),
        dest_dir,
        relative_path,
    })
}

/// Resolved decisions for a single install operation — pure, no gateway access.
pub(crate) struct InstallDecision {
    /// True when the install should be blocked because the artifact was locally
    /// modified and `--force` was not passed.
    pub blocked: bool,
    /// True when a lockfile write failure should trigger rollback of the copy.
    /// Only set for fresh installs — rolling back an existing copy is worse than
    /// the ghost we're trying to prevent.
    pub rollback_on_lock_fail: bool,
    /// True when the existing on-disk copy should be removed before copying the
    /// replacement, so local-only files do not linger after `--force`.
    pub replace_existing: bool,
}

/// Pure decision function: given pre-gathered facts, return the install decisions.
/// No gateway access — all I/O must happen in the shell before calling this.
pub(crate) fn decide_install(
    already_installed: bool,
    locally_modified: bool,
    force: bool,
) -> InstallDecision {
    InstallDecision {
        blocked: locally_modified && !force,
        rollback_on_lock_fail: !already_installed,
        replace_existing: force && already_installed,
    }
}

/// Check whether the named artifact has been locally modified since it was
/// installed. Returns `Ok(true)` when modifications are detected, `Ok(false)`
/// when clean. Gateway I/O only — the caller decides what to do with the result.
fn check_local_modifications(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    lock_entry: Option<&LockEntry>,
    ctx: &AppContext<'_>,
) -> Result<bool> {
    let dest_check = ctx.paths.require_installed_artifact_path(kind, artifact_name, scope)?;
    if ctx.fs.exists(&dest_check) {
        if let Some(entry) = lock_entry {
            return checksum::is_locally_modified(&dest_check, kind, entry, ctx.fs);
        }
    }
    Ok(false)
}

/// Pure builder: construct a `LockEntry` from plan data and pre-computed checksums.
/// The shell passes `installed_at` (an RFC 3339 timestamp); no I/O inside.
fn build_lock_entry(
    plan: &InstallPlan,
    kind: ArtifactKind,
    source_checksum: String,
    installed_checksum: String,
    installed_at: String,
) -> LockEntry {
    LockEntry {
        artifact_type: kind,
        version: plan.version.clone(),
        installed_at,
        source: LockSource {
            repo: plan.source_name.clone(),
            path: plan.relative_path.clone(),
        },
        source_checksum,
        installed_checksum,
    }
}

fn parse_name(name: &str) -> (Option<&str>, &str) {
    if let Some((source, artifact)) = name.split_once(':') {
        (Some(source), artifact)
    } else {
        (None, name)
    }
}

fn collect_discarded_paths(
    kind: ArtifactKind,
    installed_path: &Path,
    source_path: &Path,
    ctx: &AppContext<'_>,
) -> Result<Vec<PathBuf>> {
    let changes = file_changes_between(kind, installed_path, source_path, ctx)?;
    Ok(changes
        .into_iter()
        .map(|change| changed_target_path(installed_path, &change))
        .collect())
}

fn changed_target_path(installed_path: &Path, change: &FileChange) -> PathBuf {
    match change.status {
        FileStatus::Modified | FileStatus::OnlyInInstalled | FileStatus::OnlyInSource => {
            if installed_path.is_file() {
                installed_path.to_path_buf()
            } else {
                installed_path.join(&change.path)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
