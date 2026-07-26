//! `cmx skill promote` / `cmx agent promote` — push in-place edits back to the
//! canonical home.
//!
//! The mirror of [`crate::install::update`]: where `update` pulls the home copy
//! over the installed one (discarding local edits), `promote` copies the
//! **installed** copy into the home (canonicalizing the local edits) and
//! refreshes the `home`-provenance lock baselines so the artifact reads as
//! tracked again.
//!
//! This supports the common authoring loop: an assistant edits its own skill in
//! place, then you promote those edits into the home so every platform can be
//! re-projected from one canonical copy.
//!
//! Home target only. An artifact whose lock entry points at a registered git
//! source is rejected — promoting into a git working tree needs commit/push
//! handling that does not exist yet. Agents on a platform that reformats them
//! (e.g. Codex TOML) are rejected too: the installed copy is no longer the
//! canonical markdown the home holds.

use crate::error::{CliError, Result};
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::adopt::{ensure_home_source, resolve_home};
use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::copy;
use crate::diff::{FileChange, file_changes_between};
use crate::flags::RunMode;
use crate::home_provenance;
use crate::platform::Platform;
use crate::scan;
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Outcome of promoting an in-place-edited installed copy back to the
/// canonical home.
#[derive(Debug)]
pub struct PromoteResult {
    /// Name of the promoted artifact.
    pub name: String,
    /// Whether the promoted artifact is an agent or a skill.
    pub kind: ArtifactKind,
    /// The installed copy selected as the source of truth.
    pub source_path: PathBuf,
    /// Platforms whose install directory resolves to `source_path`.
    pub source_platforms: Vec<Platform>,
    /// Where the canonical copy now lives in the home.
    pub home_path: PathBuf,
    /// `true` when `--apply` was passed and the plan was executed.
    pub apply: bool,
    /// `true` when the home copy already matched the installed copy — nothing
    /// was written.
    pub already_current: bool,
    /// The version recorded for the promoted copy (from its frontmatter).
    pub version: Option<String>,
    /// Per-file changes the home will receive (or received).
    pub file_changes: Vec<FileChange>,
    /// Platforms whose `home`-provenance lock baseline was refreshed to the
    /// promoted content.
    pub retracked: Vec<Platform>,
    /// Platforms that still track this artifact from `home` but whose installed
    /// copy differs from what was just promoted — they now read as drifted and
    /// need their own reconciliation (`cmx skill sync`/`promote`).
    pub still_divergent: Vec<Platform>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Promote the installed copy of `name` into the canonical home.
///
/// When `selector` names a platform (`--from`), that platform's copy is
/// canonicalized. Otherwise the copy is chosen by **drift**: the one edited in
/// place since install. No drifted copy is a no-op; several that disagree is
/// ambiguous and asks the user to pick with `--from`.
///
/// Rejects artifacts sourced from a registered git source (home-only for now)
/// and agents whose active platform reformats them away from markdown.
pub fn promote(
    name: &str,
    kind: ArtifactKind,
    selector: Option<Platform>,
    mode: RunMode,
    ctx: &AppContext<'_>,
) -> Result<PromoteResult> {
    if kind == ArtifactKind::Agent && ctx.paths.platform.transforms_agent_to_toml() {
        return Err(CliError::PromoteTomlTransformed {
            name: name.to_string(),
        });
    }

    // Where the canonical copy lives (and its current bytes, if any), resolved up
    // front so drift-aware selection can compare candidates against it.
    let home = resolve_home(ctx)?;
    let dest_dir = home.join(kind.subdir_name());
    let home_path = kind.installed_path(name, &dest_dir, ArtifactKind::HOME_AGENT_EXT);
    let home_cs = ctx
        .fs
        .exists(&home_path)
        .then(|| checksum::checksum_artifact(&home_path, kind, ctx.fs))
        .transpose()?;

    // Choose the copy to canonicalize and the platforms whose baseline to refresh.
    // Agents are reformatted per platform, so a cross-platform byte comparison is
    // meaningless — they stay single-copy on the active platform. Skills can live
    // on several platforms, so we choose by drift.
    let (installed_path, source_platforms, scope, home_tracked) = match kind {
        ArtifactKind::Agent => select_agent_copy(name, ctx)?,
        ArtifactKind::Skill => select_skill_copy(name, selector, home_cs.as_deref(), ctx)?,
    };

    let installed_cs = checksum::checksum_artifact(&installed_path, kind, ctx.fs)?;

    let version = ctx
        .fs
        .read_to_string(&kind.content_path(&installed_path))
        .ok()
        .and_then(|c| scan::extract_version_from_content(&c));
    let file_changes = file_changes_between(kind, &home_path, &installed_path, ctx)?;

    if home_cs.as_deref() == Some(installed_cs.as_str()) {
        return Ok(PromoteResult {
            name: name.to_string(),
            kind,
            source_path: installed_path,
            source_platforms,
            home_path,
            apply: mode.is_apply(),
            already_current: true,
            version,
            file_changes,
            retracked: Vec::new(),
            still_divergent: Vec::new(),
        });
    }

    if mode.is_apply() {
        write_home_copy(kind, &home, &home_path, &dest_dir, &installed_path, ctx)?;
    }

    let still_divergent =
        planned_still_divergent(name, kind, scope, &home_tracked, &installed_cs, ctx)?;
    if mode.is_apply() {
        refresh_home_baselines(name, scope, &home_tracked, &installed_cs, version.as_deref(), ctx)?;
    }

    Ok(PromoteResult {
        name: name.to_string(),
        kind,
        source_path: installed_path,
        source_platforms,
        home_path,
        apply: mode.is_apply(),
        already_current: false,
        version,
        file_changes,
        retracked: home_tracked,
        still_divergent,
    })
}

// ---------------------------------------------------------------------------
// Copy selection
// ---------------------------------------------------------------------------

/// One physical skill copy tracked from the home, shared by ≥1 platform.
#[derive(Clone)]
struct HomeCopy {
    path: PathBuf,
    checksum: String,
    platforms: Vec<Platform>,
    /// The installed bytes differ from the lock baseline — edited in place.
    drifted: bool,
}

/// Single-copy selection for agents: the active platform's copy. Agents are
/// reformatted per platform, so there is no meaningful cross-platform copy set.
fn select_agent_copy(
    name: &str,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, Vec<Platform>, InstallScope, Vec<Platform>)> {
    let (installed_path, scope) = config::find_installed_path(
        name,
        ArtifactKind::Agent,
        ctx.fs,
        ctx.paths,
    )
    .ok_or_else(|| CliError::ArtifactNotInstalledOnDisk {
        kind: ArtifactKind::Agent,
        name: name.to_string(),
        hint: crate::suggestions::installed_artifact_hint(name, Some(ArtifactKind::Agent), ctx),
    })?;
    let home_tracked = home_tracked_platforms(name, ArtifactKind::Agent, scope, ctx)?;
    if home_tracked.is_empty() {
        return Err(CliError::Message(non_home_guidance(name, ArtifactKind::Agent, scope, ctx)?));
    }
    Ok((installed_path, vec![ctx.paths.platform], scope, home_tracked))
}

/// Drift-aware selection for skills across every home-tracked platform.
fn select_skill_copy(
    name: &str,
    selector: Option<Platform>,
    home_cs: Option<&str>,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, Vec<Platform>, InstallScope, Vec<Platform>)> {
    let (scope, copies) = resolve_home_copies(name, ctx)?;
    if copies.is_empty() {
        // Installed-but-not-home-tracked, or not installed at all: reuse the
        // pointed guidance (git-sourced → edit clone / update --force; untracked
        // → adopt; missing → not-installed).
        let (_p, s) = config::find_installed_path(name, ArtifactKind::Skill, ctx.fs, ctx.paths)
            .ok_or_else(|| CliError::ArtifactNotInstalledOnDisk {
                kind: ArtifactKind::Skill,
                name: name.to_string(),
                hint: crate::suggestions::installed_artifact_hint(
                    name,
                    Some(ArtifactKind::Skill),
                    ctx,
                ),
            })?;
        return Err(CliError::Message(non_home_guidance(name, ArtifactKind::Skill, s, ctx)?));
    }
    let selected = choose_copy(name, selector, &copies, home_cs, ctx)?;
    let home_tracked = copies.iter().flat_map(|c| c.platforms.iter().copied()).collect();
    Ok((selected.path, selected.platforms, scope, home_tracked))
}

/// The scope the skill lives at, plus one [`HomeCopy`] per distinct install
/// directory among the home-tracked platforms (the shared `.agents` dir
/// collapses several platforms into one). Global scope wins over local.
fn resolve_home_copies(name: &str, ctx: &AppContext<'_>) -> Result<(InstallScope, Vec<HomeCopy>)> {
    let managed = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    for scope in InstallScope::ALL {
        let home_tracked =
            home_provenance::home_tracked_entries(name, ArtifactKind::Skill, scope, &managed, ctx)?;
        if home_tracked.is_empty() {
            continue;
        }
        // Preserve `managed`'s order for `candidates` (it drives the physical-copy
        // iteration order below); the lookup map itself doesn't need to be ordered.
        let candidates: Vec<Platform> =
            home_tracked.iter().map(|(platform, _)| *platform).collect();
        let by_platform: std::collections::HashMap<Platform, String> = home_tracked
            .into_iter()
            .map(|(platform, entry)| (platform, entry.installed_checksum))
            .collect();
        let copies = crate::platform_copies::gather_platform_copies(
            &candidates,
            ArtifactKind::Skill,
            name,
            scope,
            ctx,
            |path, platforms| {
                let checksum = checksum::checksum_artifact(&path, ArtifactKind::Skill, ctx.fs)?;
                let drifted = platforms
                    .iter()
                    .filter_map(|p| by_platform.get(p))
                    .any(|installed_checksum| checksum != *installed_checksum);
                Ok(Some(HomeCopy {
                    path,
                    checksum,
                    platforms,
                    drifted,
                }))
            },
        )?;
        if !copies.is_empty() {
            return Ok((scope, copies));
        }
    }
    Ok((InstallScope::Global, Vec::new()))
}

/// Pick which copy to canonicalize. An explicit `--from` wins; otherwise the
/// single drifted (edited-in-place) copy is chosen. Zero drifted copies is a
/// no-op — or a refusal when the home diverged elsewhere; two or more that
/// disagree is ambiguous and asks the user to pick.
fn choose_copy(
    name: &str,
    selector: Option<Platform>,
    copies: &[HomeCopy],
    home_cs: Option<&str>,
    ctx: &AppContext<'_>,
) -> Result<HomeCopy> {
    if let Some(p) = selector {
        return copies.iter().find(|c| c.platforms.contains(&p)).cloned().ok_or_else(|| {
            CliError::Message(format!(
                "'{name}' isn't installed and home-tracked on platform '{p}'. It's \
                     home-tracked on: {}. Promote from one of those, or drop --from to \
                     auto-select the edited copy.",
                platform_list(copies)
            ))
        });
    }

    let drifted: Vec<&HomeCopy> = copies.iter().filter(|c| c.drifted).collect();
    let distinct: BTreeSet<&str> = drifted.iter().map(|c| c.checksum.as_str()).collect();
    match distinct.len() {
        0 => {
            let active = active_copy(copies, ctx.paths.platform);
            if home_cs.is_none() || home_cs == Some(active.checksum.as_str()) {
                Ok(active.clone())
            } else {
                Err(CliError::PromoteNoEdits {
                    name: name.to_string(),
                })
            }
        }
        1 => Ok((*drifted[0]).clone()),
        _ => Err(CliError::PromoteDiverging {
            name: name.to_string(),
            platforms: drifted_labels(&drifted, ctx.paths.platform),
        }),
    }
}

/// Which copy's *content* is canonical when nothing has drifted: the one read
/// by the active platform, else the first (deterministic by path).
///
/// This is a different decision from
/// [`representative_platform`](crate::platform_copies::representative_platform)
/// even though both start from "the active platform if it reads a copy, else
/// a fallback": that helper picks which *platform to name* in a message about
/// a copy that's already chosen. This picks which *copy's bytes* to treat as
/// the source of truth in the first place. Folding them into one function
/// would make a caller that only wants a display label accidentally depend on
/// content-selection semantics (and vice versa) — so they stay separate on
/// purpose.
fn active_copy(copies: &[HomeCopy], active: Platform) -> &HomeCopy {
    copies.iter().find(|c| c.platforms.contains(&active)).unwrap_or(&copies[0])
}

/// Comma-joined platform names across all copies, for guidance messages.
fn platform_list(copies: &[HomeCopy]) -> String {
    copies
        .iter()
        .flat_map(|c| c.platforms.iter())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// One representative platform label per drifted copy, for the ambiguity
/// message — `managed` is `None` here because `resolve_home_copies` already
/// restricted candidates to managed platforms, so filtering again would be a
/// no-op.
fn drifted_labels(drifted: &[&HomeCopy], active: Platform) -> String {
    drifted
        .iter()
        .filter_map(|c| crate::platform_copies::representative_platform(&c.platforms, active, None))
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Replace the home copy with the installed one (remove first so files deleted
/// from the installed copy don't linger in the home).
fn write_home_copy(
    kind: ArtifactKind,
    home: &std::path::Path,
    home_path: &std::path::Path,
    dest_dir: &std::path::Path,
    installed_path: &std::path::Path,
    ctx: &AppContext<'_>,
) -> Result<()> {
    ensure_home_source(home, ctx)?;
    if ctx.fs.exists(home_path) {
        crate::uninstall::remove_installed(kind, home_path, ctx.fs)?;
    }
    ctx.fs.create_dir_all(dest_dir)?;
    copy::copy_artifact_to(kind, installed_path, dest_dir, ctx.fs)?;
    Ok(())
}

/// Platforms that would still differ from the promoted content after the home
/// baselines are refreshed.
fn planned_still_divergent(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    home_tracked: &[Platform],
    installed_cs: &str,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    let mut still_divergent = Vec::new();
    for &platform in home_tracked {
        let pv = ctx.paths.with_platform(platform);
        if let Some(p) = pv.installed_artifact_path(kind, name, scope)
            && ctx.fs.exists(&p)
            && checksum::checksum_artifact(&p, kind, ctx.fs)? != installed_cs
        {
            still_divergent.push(platform);
        }
    }
    Ok(still_divergent)
}

/// Refresh every home-provenance lock baseline to the promoted content.
///
/// Unlike `sync`'s equivalent, promote also refreshes `source_checksum`: the
/// home *is* the source for a home-provenance artifact, so canonicalizing the
/// installed copy into it means the source baseline now matches too.
fn refresh_home_baselines(
    name: &str,
    scope: InstallScope,
    home_tracked: &[Platform],
    installed_cs: &str,
    version: Option<&str>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let now = ctx.clock.now().to_rfc3339();
    crate::lock_baseline::refresh_baseline(
        name,
        scope,
        home_tracked,
        &crate::lock_baseline::BaselineUpdate {
            checksum: installed_cs,
            version,
            source_checksum: Some(installed_cs),
            now: &now,
        },
        ctx,
    )
}

/// Platforms whose lock entry for `name` (at `scope`) records `home` provenance.
fn home_tracked_platforms(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    let candidates = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    Ok(home_provenance::home_tracked_entries(name, kind, scope, &candidates, ctx)?
        .into_iter()
        .map(|(platform, _)| platform)
        .collect())
}

/// Build a pointed error for an artifact that isn't tracked from `home`,
/// distinguishing a git-sourced one (edit the clone / `update --force`) from an
/// untracked/orphaned one (`adopt`/`install`).
fn non_home_guidance(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<String> {
    let candidates = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    if let Some((_, entry)) =
        home_provenance::lock_entries_for(name, kind, scope, &candidates, ctx)?
            .into_iter()
            .next()
    {
        return Ok(format!(
            "'{name}' is tracked from the '{repo}' source, not the home. Promoting edits into a \
             registered source isn't supported yet — edit the source clone directly, or run \
             `cmx {kind} update {name} --force` to discard the local edits.",
            repo = entry.source.repo
        ));
    }
    Ok(format!(
        "'{name}' isn't tracked by cmx, so there's nothing to promote it into. If it's \
         hand-authored, bring it into the home with `cmx {kind} adopt {name}`; if a registered \
         source provides it, run `cmx {kind} install {name}`."
    ))
}

#[cfg(test)]
#[path = "promote/tests.rs"]
mod tests;
