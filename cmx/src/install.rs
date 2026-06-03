use anyhow::{Result, bail};
use std::path::PathBuf;

use crate::checksum;
use crate::context::AppContext;
use crate::copy;
use crate::lockfile;
use crate::partition::{Partitioned, partition_by};
use crate::paths::ConfigPaths;
use crate::source_iter;
use crate::source_update;
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

    source_update::ensure_fresh(ctx)?;

    let found = source_iter::find_unique(artifact_name, kind, source_name, ctx)?;

    let plan = plan_install(artifact_name, kind, scope, &found, ctx.paths);

    ctx.fs.create_dir_all(&plan.dest_dir)?;

    let source_checksum = checksum::checksum_artifact(&found.artifact.path, kind, ctx.fs)?;

    // Check for local modifications before overwriting
    if !force {
        let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
        check_local_modifications(
            artifact_name,
            kind,
            scope,
            lock.packages.get(artifact_name),
            ctx,
        )?;
    }

    // Record whether this is a fresh install (vs. an update/reinstall) so that
    // we can roll back if the lockfile write fails.
    let already_installed = ctx.paths.is_installed(kind, artifact_name, scope, ctx.fs);

    let dest_path =
        copy::copy_artifact(&found.artifact.path, &plan.dest_dir, kind, artifact_name, ctx)?;
    let installed_checksum = checksum::checksum_artifact(&dest_path, kind, ctx.fs)?;

    let lock_result = lockfile::mutate(scope, ctx.fs, ctx.paths, |lock| {
        lock.packages.insert(
            artifact_name.to_string(),
            build_lock_entry(
                &plan,
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
        if should_rollback(already_installed) {
            let _ = match kind {
                types::ArtifactKind::Agent => ctx.fs.remove_file(&dest_path),
                types::ArtifactKind::Skill => ctx.fs.remove_dir_all(&dest_path),
            };
        }
        return Err(lock_err);
    }

    Ok(InstallResult {
        artifact_name: artifact_name.to_string(),
        version: plan.version,
        kind,
        source_name: plan.source_name,
        dest_dir: plan.dest_dir,
    })
}

/// Install several named artifacts in one pass. Best-effort: each name is
/// installed independently; failures (not found, ambiguous, locally modified
/// without `--force`) are collected with their reason rather than aborting the
/// batch. Backs `cmx {skill,agent} install <name>...`.
pub fn install_many(
    names: &[String],
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallManyResult> {
    let (installed, failed) = partition_by(names, |name| {
        Ok(match install(name, kind, scope, force, ctx) {
            Ok(r) => Partitioned::Kept(r),
            Err(e) => Partitioned::Excluded((name.to_string(), e.to_string())),
        })
    })?;
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
            "No installed {kind} named '{name}' found. Install it first with 'cmx {kind} install {name}'."
        );
    };
    let pinned = format!("{}:{}", entry.source.repo, name);
    install(&pinned, kind, scope, force, ctx)
}

pub fn install_all(
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<BatchInstallResult> {
    ctx.paths.ensure_supports(kind)?;

    source_update::ensure_fresh(ctx)?;

    let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
    let mut installed = Vec::new();

    for sa in source_iter::all_artifacts(ctx)? {
        if sa.artifact.kind != kind {
            continue;
        }
        // Skip if already tracked with matching version AND checksum
        if let Some(lock_entry) = lock.packages.get(&sa.artifact.name) {
            let source_cs = checksum::checksum_artifact(&sa.artifact.path, kind, ctx.fs)?;
            if lock_entry.version.as_deref() == sa.artifact.version.as_deref()
                && lock_entry.source_checksum == source_cs
            {
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

    source_update::ensure_fresh(ctx)?;

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
                    si.source_name == entry.source.repo && si.checksum != entry.source_checksum
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

/// Compute the destination directory and relative source path for an install.
/// Pure function — no filesystem access.
fn plan_install(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    found: &source_iter::SourceArtifact,
    paths: &ConfigPaths,
) -> InstallPlan {
    let dest_dir = paths.install_dir(kind, scope);
    let relative_path = types::relative_path_string(&found.artifact.path, &found.source_root);
    InstallPlan {
        artifact_name: artifact_name.to_string(),
        version: found.artifact.version.clone(),
        source_name: found.source_name.clone(),
        source_root: found.source_root.clone(),
        dest_dir,
        relative_path,
    }
}

/// Check whether the named artifact has been locally modified since it was
/// installed. Returns `Ok(())` if clean, or bails with a user-facing error if
/// modifications are detected.
fn check_local_modifications(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    lock_entry: Option<&LockEntry>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let dest_check = ctx.paths.installed_artifact_path(kind, artifact_name, scope);
    if ctx.fs.exists(&dest_check) {
        if let Some(entry) = lock_entry {
            if checksum::is_locally_modified(&dest_check, kind, entry, ctx.fs)? {
                bail!(
                    "'{artifact_name}' has local modifications. Use --force to overwrite, \
                     or 'cmx {kind} diff {artifact_name}' to review changes first."
                );
            }
        }
    }
    Ok(())
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

/// Pure predicate: should we roll back the copied artifact when a lock write fails?
/// Only true for a *fresh* install — rolling back an existing artifact would discard
/// the user's current copy, which is worse than the ghost we're trying to prevent.
fn should_rollback(already_installed: bool) -> bool {
    !already_installed
}

fn parse_name(name: &str) -> (Option<&str>, &str) {
    if let Some((source, artifact)) = name.split_once(':') {
        (Some(source), artifact)
    } else {
        (None, name)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
