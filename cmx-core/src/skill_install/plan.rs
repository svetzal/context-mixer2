use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::checksum;
use crate::context::AppContext;
use crate::fs_util;
use crate::skill_fs::{self, SkillFile};
use crate::types::{ArtifactKind, LockEntry, LockSource};

use super::{InstallPlan, PreparedWrites, TargetAction, ToolIdentity};

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compare two semver version strings.
///
/// - `None` installed → `Less` (treat as "not installed").
/// - Both parse → standard semver comparison.
/// - Either parse fails → string equality: `Equal` if equal, else `Less`.
fn compare_versions(installed: Option<&str>, bundled: &str) -> Ordering {
    let Some(inst) = installed else {
        return Ordering::Less;
    };
    match (semver::Version::parse(inst), semver::Version::parse(bundled)) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => {
            if inst == bundled {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        }
    }
}

/// Decide what action to take for a platform that already has a lock entry.
pub(super) fn decide_action_for_entry(
    entry: &LockEntry,
    bundled_version: &str,
    source_checksum: &str,
    force: bool,
    skill_dest: &std::path::Path,
    ctx: &AppContext<'_>,
) -> Result<TargetAction> {
    let installed_version = entry.version.as_deref();
    let cmp = compare_versions(installed_version, bundled_version);

    match cmp {
        Ordering::Less => Ok(TargetAction::Update {
            from: installed_version.map(str::to_string),
        }),
        Ordering::Equal => {
            if !ctx.fs.exists(skill_dest) {
                return Ok(TargetAction::Install);
            }

            let disk_checksum =
                checksum::checksum_artifact(skill_dest, ArtifactKind::Skill, ctx.fs)?;
            if disk_checksum == source_checksum {
                Ok(TargetAction::Skip)
            } else if force {
                Ok(TargetAction::Update {
                    from: installed_version.map(str::to_string),
                })
            } else {
                Ok(TargetAction::DriftedSkip {
                    installed: installed_version.unwrap_or("unknown").to_string(),
                })
            }
        }
        Ordering::Greater => {
            if force {
                Ok(TargetAction::Downgrade {
                    from: installed_version.unwrap_or("unknown").to_string(),
                })
            } else {
                Ok(TargetAction::RefuseNewer {
                    installed: installed_version.unwrap_or("unknown").to_string(),
                })
            }
        }
    }
}

fn discarded_paths_against_bundle(
    skill_dest: &std::path::Path,
    bundled_files: &[SkillFile],
    ctx: &AppContext<'_>,
) -> Result<Vec<std::path::PathBuf>> {
    if !ctx.fs.exists(skill_dest) {
        return Ok(Vec::new());
    }

    let installed_files = fs_util::collect_files_recursive(skill_dest, ctx.fs)?;
    let mut installed_by_rel = BTreeMap::new();
    for path in installed_files {
        let rel = path.strip_prefix(skill_dest).unwrap_or(&path).to_path_buf();
        installed_by_rel.insert(rel, ctx.fs.read(&path)?);
    }

    let mut bundled_by_rel = BTreeMap::new();
    for file in skill_fs::canonical_files(bundled_files) {
        bundled_by_rel.insert(file.rel_path.clone(), file.bytes.clone());
    }

    let mut changed_paths = Vec::new();
    let relative_paths: BTreeSet<_> =
        installed_by_rel.keys().chain(bundled_by_rel.keys()).cloned().collect();

    for rel_path in relative_paths {
        match (installed_by_rel.get(&rel_path), bundled_by_rel.get(&rel_path)) {
            (Some(installed), Some(bundled)) if installed == bundled => {}
            (Some(_) | None, Some(_)) | (Some(_), None) => {
                changed_paths.push(skill_dest.join(rel_path));
            }
            (None, None) => {}
        }
    }

    Ok(changed_paths)
}

pub(super) fn prepare_writes(
    plan: &InstallPlan,
    files: &[SkillFile],
    ctx: &AppContext<'_>,
) -> Result<PreparedWrites> {
    let mut dirs_to_write = BTreeSet::new();
    let mut dirs_to_replace = BTreeSet::new();

    for target in &plan.targets {
        if target.action.will_write() {
            dirs_to_write.insert(target.dest_dir.clone());
        }
        if plan.force
            && matches!(target.action, TargetAction::Update { .. } | TargetAction::Downgrade { .. })
        {
            dirs_to_replace.insert(target.dest_dir.clone());
        }
    }

    let mut discarded_paths_by_dir = BTreeMap::new();
    for dir in &dirs_to_replace {
        discarded_paths_by_dir
            .insert(dir.clone(), discarded_paths_against_bundle(dir, files, ctx)?);
        if ctx.fs.exists(dir) {
            ctx.fs.remove_dir_all(dir)?;
        }
    }

    Ok(PreparedWrites {
        dirs_to_write,
        discarded_paths_by_dir,
    })
}

pub(super) fn build_lock_entry(
    tool: &ToolIdentity,
    checksum: &str,
    installed_at: &str,
) -> LockEntry {
    LockEntry {
        artifact_type: ArtifactKind::Skill,
        version: Some(tool.version.clone()),
        installed_at: installed_at.to_string(),
        source: LockSource {
            repo: format!("bundled:{}", tool.name),
            path: format!("skills/{}", tool.name),
        },
        source_checksum: checksum.to_string(),
        installed_checksum: checksum.to_string(),
    }
}
