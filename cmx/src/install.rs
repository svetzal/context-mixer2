use crate::error::{CliError, Result};
use crate::flags::Force;
use std::collections::BTreeMap;
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
use crate::types::{self, ArtifactKind, InstallScope, LockEntry, LockSource, SourcesFile};

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

#[derive(Debug)]
pub struct UpdateResult {
    pub updated: InstallResult,
    pub sibling_drifted_platforms: Vec<Platform>,
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
    force: Force,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    let sources = crate::config::load_sources(ctx.fs, ctx.paths)?;
    let (source_name, artifact_name) = parse_name_with_sources(name, &sources);
    install_resolved(source_name, artifact_name, kind, scope, force, ctx)
}

fn install_resolved(
    source_name: Option<&str>,
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    force: Force,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    ctx.paths.ensure_supports(kind)?;

    let found = source_iter::find_unique(artifact_name, kind, source_name, ctx)?;
    let plan = plan_install(artifact_name, kind, scope, &found, ctx.paths)?;
    ctx.fs.create_dir_all(&plan.dest_dir)?;
    let source_checksum = checksum::checksum_artifact(&found.artifact.path, kind, ctx.fs)?;

    let facts = gather_install_facts(artifact_name, kind, scope, force, ctx)?;

    // Version guard: refuse to downgrade a newer-installed copy unless forced.
    if facts.already_installed && !force.is_yes() {
        let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
        if let Some(entry) = lock.packages.get(artifact_name) {
            if artifact_status::installed_is_newer(
                entry.version.as_deref(),
                plan.version.as_deref(),
            ) {
                return Err(CliError::InstalledNewerThanSource {
                    name: artifact_name.to_string(),
                    installed: entry.version.clone().unwrap_or_default(),
                    source_version: plan.version.clone().unwrap_or_default(),
                });
            }
        }
    }

    let decision = decide_install(facts.already_installed, facts.locally_modified, force);
    if decision.blocked {
        return Err(CliError::LocallyModified {
            name: artifact_name.to_string(),
            kind,
        });
    }
    let discarded_paths = if force.is_yes() && facts.locally_modified {
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
    Ok(crate::targets::resolve_targets(selector, kind, scope, ctx)?)
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
    force: Force,
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
    force: Force,
    ctx: &AppContext<'_>,
) -> Result<UpdateResult> {
    let Some((entry, scope)) = lockfile::find_entry(name, ctx.fs, ctx.paths)? else {
        return Err(CliError::ArtifactNotInstalled {
            kind,
            name: name.to_string(),
            hint: crate::suggestions::installed_artifact_hint(name, Some(kind), ctx),
        });
    };
    let updated = install_resolved(Some(&entry.source.repo), name, kind, scope, force, ctx)?;
    let sibling_drifted_platforms = drifted_sibling_platforms(name, kind, scope, &updated, ctx)?;
    Ok(UpdateResult {
        updated,
        sibling_drifted_platforms,
    })
}

/// Install every available artifact of `kind` from the sources into each of
/// `targets`, concatenating the per-platform results.
pub fn install_all(
    kind: ArtifactKind,
    scope: InstallScope,
    force: Force,
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
    force: Force,
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
        let result =
            install_resolved(Some(&sa.source_name), &sa.artifact.name, kind, scope, force, ctx)?;
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
    force: Force,
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
                let result =
                    install_resolved(Some(&entry.source.repo), name, kind, *scope, force, ctx)?;
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
    force: Force,
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
        locally_modified: locally_modified && (!force.is_yes() || already_installed),
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
        return Err(lock_err.into());
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
///
/// `already_installed` and `locally_modified` are value-carrying state
/// predicates and stay as `bool`; `force` is an intent flag and takes
/// [`Force`] to avoid boolean blindness at call sites.
pub(crate) fn decide_install(
    already_installed: bool,
    locally_modified: bool,
    force: Force,
) -> InstallDecision {
    InstallDecision {
        blocked: locally_modified && !force.is_yes(),
        rollback_on_lock_fail: !already_installed,
        replace_existing: force.is_yes() && already_installed,
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
            return Ok(checksum::is_locally_modified(&dest_check, kind, entry, ctx.fs)?);
        }
    }
    Ok(false)
}

fn drifted_sibling_platforms(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    updated: &InstallResult,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    let mut sibling_paths: BTreeMap<PathBuf, Vec<Platform>> = BTreeMap::new();
    for platform in crate::config::managed_or_all_platforms(ctx.fs, ctx.paths)? {
        if platform == updated.platform || !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        let Some(path) = pv.installed_artifact_path(kind, artifact_name, scope) else {
            continue;
        };
        if !ctx.fs.exists(&path) {
            continue;
        }
        sibling_paths.entry(path).or_default().push(platform);
    }

    let updated_checksum = if kind == ArtifactKind::Skill {
        let updated_path = ctx.paths.require_installed_artifact_path(kind, artifact_name, scope)?;
        Some(checksum::checksum_artifact(&updated_path, kind, ctx.fs)?)
    } else {
        None
    };

    let mut drifted = Vec::new();
    for (path, platforms) in sibling_paths {
        let mut tracked_platforms = Vec::new();
        let mut lock_entry = None;
        for platform in platforms {
            let pv = ctx.paths.with_platform(platform);
            if let Some(entry) = lockfile::load(scope, ctx.fs, &pv)?.packages.get(artifact_name)
                && entry.artifact_type == kind
            {
                tracked_platforms.push(platform);
                lock_entry.get_or_insert_with(|| entry.clone());
            }
        }
        if tracked_platforms.is_empty() {
            continue;
        }

        let sibling_checksum = checksum::checksum_artifact(&path, kind, ctx.fs)?;
        let lock_drifted =
            lock_entry.is_some_and(|entry| entry.installed_checksum != sibling_checksum);
        let diverged_from_updated =
            updated_checksum.as_ref().is_some_and(|checksum| checksum != &sibling_checksum);
        if lock_drifted || diverged_from_updated {
            drifted.extend(tracked_platforms);
        }
    }

    Ok(drifted)
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
    LockEntry::new(
        kind,
        plan.version.clone(),
        LockSource::new(&plan.source_name, &plan.relative_path),
        source_checksum,
        installed_checksum,
        installed_at,
    )
}

fn parse_name_with_sources<'a>(name: &'a str, sources: &SourcesFile) -> (Option<&'a str>, &'a str) {
    let mut resolved: Option<(Option<&'a str>, &'a str)> = None;
    for (idx, _) in name.match_indices(':') {
        let candidate = &name[..idx];
        if sources.sources.contains_key(candidate) {
            resolved = Some((Some(candidate), &name[idx + 1..]));
        }
    }

    resolved.unwrap_or((None, name))
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
