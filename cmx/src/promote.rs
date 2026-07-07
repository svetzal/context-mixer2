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

use anyhow::{Context, Result, anyhow, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::adopt::{HOME_SOURCE, ensure_home_source, resolve_home};
use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::copy;
use crate::diff::{FileChange, file_changes_between};
use crate::lockfile;
use crate::platform::Platform;
use crate::platform_iter;
use crate::scan;
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct PromoteResult {
    pub name: String,
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
    apply: bool,
    ctx: &AppContext<'_>,
) -> Result<PromoteResult> {
    if kind == ArtifactKind::Agent && ctx.paths.platform.transforms_agent_to_toml() {
        bail!(
            "Can't promote '{name}': the active platform stores agents as transformed TOML, not \
             the canonical markdown the home holds. Promote from a markdown platform (e.g. claude)."
        );
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
        return Ok(already_current_result(
            name,
            kind,
            installed_path,
            source_platforms,
            home_path,
            apply,
            version,
            file_changes,
        ));
    }

    if apply {
        write_home_copy(kind, &home, &home_path, &dest_dir, &installed_path, ctx)?;
    }

    let still_divergent =
        planned_still_divergent(name, kind, scope, &home_tracked, &installed_cs, ctx)?;
    if apply {
        refresh_home_baselines(name, scope, &home_tracked, &installed_cs, version.as_deref(), ctx)?;
    }

    Ok(promoted_result(
        name,
        kind,
        installed_path,
        source_platforms,
        home_path,
        apply,
        version,
        file_changes,
        home_tracked,
        still_divergent,
    ))
}

// ---------------------------------------------------------------------------
// Result assembly helpers
// ---------------------------------------------------------------------------

/// Build a `PromoteResult` for the case where the home copy is already
/// identical to the installed copy — nothing to write.
#[allow(clippy::too_many_arguments)]
fn already_current_result(
    name: &str,
    kind: ArtifactKind,
    installed_path: PathBuf,
    source_platforms: Vec<Platform>,
    home_path: PathBuf,
    apply: bool,
    version: Option<String>,
    file_changes: Vec<FileChange>,
) -> PromoteResult {
    PromoteResult {
        name: name.to_string(),
        kind,
        source_path: installed_path,
        source_platforms,
        home_path,
        apply,
        already_current: true,
        version,
        file_changes,
        retracked: Vec::new(),
        still_divergent: Vec::new(),
    }
}

/// Build a `PromoteResult` for the case where the installed copy was promoted
/// into the home.
#[allow(clippy::too_many_arguments)]
fn promoted_result(
    name: &str,
    kind: ArtifactKind,
    installed_path: PathBuf,
    source_platforms: Vec<Platform>,
    home_path: PathBuf,
    apply: bool,
    version: Option<String>,
    file_changes: Vec<FileChange>,
    home_tracked: Vec<Platform>,
    still_divergent: Vec<Platform>,
) -> PromoteResult {
    PromoteResult {
        name: name.to_string(),
        kind,
        source_path: installed_path,
        source_platforms,
        home_path,
        apply,
        already_current: false,
        version,
        file_changes,
        retracked: home_tracked,
        still_divergent,
    }
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
    .with_context(|| {
        format!(
            "No installed agent named '{name}' found on disk. {}",
            crate::suggestions::installed_artifact_hint(name, Some(ArtifactKind::Agent), ctx)
        )
    })?;
    let home_tracked = home_tracked_platforms(name, ArtifactKind::Agent, scope, ctx)?;
    if home_tracked.is_empty() {
        bail!(non_home_guidance(name, ArtifactKind::Agent, scope, ctx)?);
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
            .with_context(|| {
                format!(
                    "No installed skill named '{name}' found on disk. {}",
                    crate::suggestions::installed_artifact_hint(
                        name,
                        Some(ArtifactKind::Skill),
                        ctx
                    )
                )
            })?;
        bail!(non_home_guidance(name, ArtifactKind::Skill, s, ctx)?);
    }
    let selected = choose_copy(name, selector, &copies, home_cs, ctx)?;
    let home_tracked = copies.iter().flat_map(|c| c.platforms.iter().copied()).collect();
    Ok((selected.path, selected.platforms, scope, home_tracked))
}

/// The scope the skill lives at, plus one [`HomeCopy`] per distinct install
/// directory among the home-tracked platforms (the shared `.agents` dir
/// collapses several platforms into one). Global scope wins over local.
fn resolve_home_copies(name: &str, ctx: &AppContext<'_>) -> Result<(InstallScope, Vec<HomeCopy>)> {
    for scope in InstallScope::ALL {
        let mut by_dir: BTreeMap<PathBuf, HomeCopy> = BTreeMap::new();
        for view in platform_iter::views_for(ctx.paths, platform_iter::all(), ArtifactKind::Skill) {
            let lock = lockfile::load(scope, ctx.fs, &view.paths)?;
            let Some(entry) = lock.packages.get(name) else {
                continue;
            };
            if entry.source.repo != HOME_SOURCE {
                continue;
            }
            let Some(path) = view.paths.installed_artifact_path(ArtifactKind::Skill, name, scope)
            else {
                continue;
            };
            if !ctx.fs.exists(&path) {
                continue;
            }
            if let Some(existing) = by_dir.get_mut(&path) {
                existing.platforms.push(view.platform);
                existing.drifted |= existing.checksum != entry.installed_checksum;
            } else {
                let checksum = checksum::checksum_artifact(&path, ArtifactKind::Skill, ctx.fs)?;
                let drifted = checksum != entry.installed_checksum;
                by_dir.insert(
                    path.clone(),
                    HomeCopy {
                        path,
                        checksum,
                        platforms: vec![view.platform],
                        drifted,
                    },
                );
            }
        }
        if !by_dir.is_empty() {
            return Ok((scope, by_dir.into_values().collect()));
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
            anyhow!(
                "'{name}' isn't installed and home-tracked on platform '{p}'. It's \
                     home-tracked on: {}. Promote from one of those, or drop --from to \
                     auto-select the edited copy.",
                platform_list(copies)
            )
        });
    }

    let drifted: Vec<&HomeCopy> = copies.iter().filter(|c| c.drifted).collect();
    let distinct: BTreeSet<&str> = drifted.iter().map(|c| c.checksum.as_str()).collect();
    match distinct.len() {
        0 => {
            let rep = representative(copies, ctx.paths.platform);
            if home_cs.is_none() || home_cs == Some(rep.checksum.as_str()) {
                Ok(rep.clone())
            } else {
                bail!(
                    "No in-place edits detected on any platform — nothing to promote. The home \
                     already differs from the installed copies (it was changed elsewhere). Run \
                     `cmx skill update {name} --force` to pull the home over the installs, or \
                     `cmx skill promote {name} --from <name>` to force a specific copy into \
                     the home."
                )
            }
        }
        1 => Ok((*drifted[0]).clone()),
        _ => bail!(
            "Multiple platforms have diverging in-place edits: {}. cmx can't tell which should \
             become the canonical home copy. Inspect them with `cmx skill diff {name}`, then \
             promote the one you want with `cmx skill promote {name} --from <name>`.",
            drifted_labels(&drifted, ctx.paths.platform)
        ),
    }
}

/// The copy read by the active platform, else the first (deterministic by path).
fn representative(copies: &[HomeCopy], active: Platform) -> &HomeCopy {
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

/// One representative platform label per drifted copy, for the ambiguity message
/// (the active platform when it reads a copy, else that copy's first platform).
fn drifted_labels(drifted: &[&HomeCopy], active: Platform) -> String {
    drifted
        .iter()
        .map(|c| {
            if c.platforms.contains(&active) {
                active.to_string()
            } else {
                c.platforms[0].to_string()
            }
        })
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
fn refresh_home_baselines(
    name: &str,
    scope: InstallScope,
    home_tracked: &[Platform],
    installed_cs: &str,
    version: Option<&str>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let now = ctx.clock.now().to_rfc3339();
    for &platform in home_tracked {
        let pv = ctx.paths.with_platform(platform);
        lockfile::mutate(scope, ctx.fs, &pv, |lock| {
            if let Some(entry) = lock.packages.get_mut(name) {
                entry.source_checksum = installed_cs.to_string();
                entry.installed_checksum = installed_cs.to_string();
                entry.version = version.map(str::to_string);
                entry.installed_at.clone_from(&now);
            }
        })?;
    }
    Ok(())
}

/// Platforms whose lock entry for `name` (at `scope`) records `home` provenance.
fn home_tracked_platforms(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    let mut platforms = Vec::new();
    for view in platform_iter::views_for(ctx.paths, platform_iter::all(), kind) {
        let tracked_from_home = lockfile::load(scope, ctx.fs, &view.paths)?
            .packages
            .get(name)
            .is_some_and(|e| e.source.repo == HOME_SOURCE);
        if tracked_from_home {
            platforms.push(view.platform);
        }
    }
    Ok(platforms)
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
    for view in platform_iter::views_for(ctx.paths, platform_iter::all(), kind) {
        if let Some(entry) = lockfile::load(scope, ctx.fs, &view.paths)?.packages.get(name) {
            return Ok(format!(
                "'{name}' is tracked from the '{repo}' source, not the home. Promoting edits into a \
                 registered source isn't supported yet — edit the source clone directly, or run \
                 `cmx {kind} update {name} --force` to discard the local edits.",
                repo = entry.source.repo
            ));
        }
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
