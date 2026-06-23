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

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::adopt::{HOME_SOURCE, ensure_home_source, resolve_home};
use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::copy;
use crate::lockfile;
use crate::platform::Platform;
use crate::scan;
use crate::types::{ArtifactKind, InstallScope};

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct PromoteResult {
    pub name: String,
    pub kind: ArtifactKind,
    /// Where the canonical copy now lives in the home.
    pub home_path: PathBuf,
    /// `true` when the home copy already matched the installed copy — nothing
    /// was written.
    pub already_current: bool,
    /// The version recorded for the promoted copy (from its frontmatter).
    pub version: Option<String>,
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
/// Operates on the copy `cmx diff` shows — global scope preferred, then local.
/// Rejects artifacts sourced from a registered git source (home-only for now)
/// and agents whose active platform reformats them away from markdown.
pub fn promote(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<PromoteResult> {
    if kind == ArtifactKind::Agent && ctx.paths.platform.transforms_agent_to_toml() {
        bail!(
            "Can't promote '{name}': the active platform stores agents as transformed TOML, not \
             the canonical markdown the home holds. Promote from a markdown platform (e.g. claude)."
        );
    }

    // The installed copy to canonicalize — the same one `diff` compares.
    let (installed_path, scope) = config::find_installed_path(name, kind, ctx.fs, ctx.paths)
        .with_context(|| format!("No installed {kind} named '{name}' found on disk."))?;

    // Promote targets the home, so the artifact must already be tracked from
    // `home`. A git-sourced or untracked artifact is steered elsewhere.
    let home_tracked = home_tracked_platforms(name, kind, scope, ctx)?;
    if home_tracked.is_empty() {
        bail!(non_home_guidance(name, kind, scope, ctx)?);
    }

    let home = resolve_home(ctx)?;
    ensure_home_source(&home, ctx)?;
    let dest_dir = home.join(kind.subdir_name());
    let home_path = kind.installed_path(name, &dest_dir, ArtifactKind::HOME_AGENT_EXT);

    let installed_cs = checksum::checksum_artifact(&installed_path, kind, ctx.fs)?;
    let home_cs = ctx
        .fs
        .exists(&home_path)
        .then(|| checksum::checksum_artifact(&home_path, kind, ctx.fs))
        .transpose()?;

    let version = ctx
        .fs
        .read_to_string(&kind.content_path(&installed_path))
        .ok()
        .and_then(|c| scan::extract_version_from_content(&c));

    if home_cs.as_deref() == Some(installed_cs.as_str()) {
        return Ok(PromoteResult {
            name: name.to_string(),
            kind,
            home_path,
            already_current: true,
            version,
            retracked: Vec::new(),
            still_divergent: Vec::new(),
        });
    }

    // Replace the home copy with the installed one (remove first so files
    // deleted from the installed copy don't linger in the home).
    if ctx.fs.exists(&home_path) {
        crate::uninstall::remove_installed(kind, &home_path, ctx.fs)?;
    }
    ctx.fs.create_dir_all(&dest_dir)?;
    copy::copy_artifact_to(kind, &installed_path, &dest_dir, ctx.fs)?;

    // Refresh every home-provenance lock baseline to the promoted content. A
    // platform whose installed copy matches becomes tracked; one that still
    // differs reads as drifted afterwards (truthfully — it diverges from the
    // freshly promoted home).
    let now = ctx.clock.now().to_rfc3339();
    let mut still_divergent = Vec::new();
    for &platform in &home_tracked {
        let pv = ctx.paths.with_platform(platform);
        if let Some(p) = pv.installed_artifact_path(kind, name, scope) {
            if ctx.fs.exists(&p) && checksum::checksum_artifact(&p, kind, ctx.fs)? != installed_cs {
                still_divergent.push(platform);
            }
        }
        lockfile::mutate(scope, ctx.fs, &pv, |lock| {
            if let Some(entry) = lock.packages.get_mut(name) {
                entry.source_checksum.clone_from(&installed_cs);
                entry.installed_checksum.clone_from(&installed_cs);
                entry.version.clone_from(&version);
                entry.installed_at.clone_from(&now);
            }
        })?;
    }

    Ok(PromoteResult {
        name: name.to_string(),
        kind,
        home_path,
        already_current: false,
        version,
        retracked: home_tracked,
        still_divergent,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Platforms whose lock entry for `name` (at `scope`) records `home` provenance.
fn home_tracked_platforms(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    let mut platforms = Vec::new();
    for platform in Platform::ALL {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        let tracked_from_home = lockfile::load(scope, ctx.fs, &pv)?
            .packages
            .get(name)
            .is_some_and(|e| e.source.repo == HOME_SOURCE);
        if tracked_from_home {
            platforms.push(platform);
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
    for platform in Platform::ALL {
        if !platform.supports(kind) {
            continue;
        }
        let pv = ctx.paths.with_platform(platform);
        if let Some(entry) = lockfile::load(scope, ctx.fs, &pv)?.packages.get(name) {
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
