//! `cmx set` — definitions, curation, and the activation lifecycle for sets.
//!
//! A set is a locally-defined, named group of installed artifacts with a
//! desired activation state (see `SETS.md`). `create`, `list`, `show`, `add`,
//! `remove`, `rename` are pure curation with no install/uninstall side
//! effects. `activate`/`deactivate` (Phase 2) compose the existing
//! `install`/`uninstall` machinery to make that state actionable; `delete
//! --purge` deactivates first. Context-footprint reporting and `doctor`
//! integration are Phase 3.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::config;
use crate::context::AppContext;
use crate::install;
use crate::lockfile;
use crate::platform_iter;
use crate::scan_marketplace::{PluginScan, scan_marketplace_plugin};
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope, SetDef, SetMember, SetState, SetsFile};
use crate::uninstall;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SetCreateResult {
    pub name: String,
    pub member_count: usize,
    /// The `<source>:<plugin>` spec the set was seeded from, if `--from` was used.
    pub seeded_from: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SetListEntry {
    pub name: String,
    pub state: SetState,
    pub member_count: usize,
    /// Total character count of the set's members' trigger descriptions — the
    /// context-footprint the set costs when active (see `SETS.md`,
    /// "Context-footprint reporting"). Members whose description could not be
    /// resolved contribute 0.
    pub footprint_chars: usize,
}

#[derive(Debug, Serialize)]
pub struct SetListResult {
    pub entries: Vec<SetListEntry>,
}

#[derive(Debug, Serialize)]
pub struct SetMemberStatus {
    pub kind: ArtifactKind,
    pub name: String,
    pub source: Option<String>,
    pub installed: bool,
    /// This member's trigger-description character count, or `None` when it
    /// could not be resolved (source missing, artifact not found).
    pub footprint_chars: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SetShowResult {
    pub name: String,
    pub description: Option<String>,
    pub state: SetState,
    pub members: Vec<SetMemberStatus>,
    /// Sum of every resolvable member's `footprint_chars`.
    pub footprint_chars: usize,
}

#[derive(Debug)]
pub struct SetAddResult {
    pub set: String,
    pub added: Vec<String>,
    pub already: Vec<String>,
}

#[derive(Debug)]
pub struct SetRemoveResult {
    pub set: String,
    pub removed: Vec<String>,
    pub not_found: Vec<String>,
}

#[derive(Debug)]
pub struct SetDeleteResult {
    pub name: String,
}

#[derive(Debug)]
pub struct SetRenameResult {
    pub old: String,
    pub new: String,
}

/// Per-member outcome of an `activate` (or `activate --dry-run`) run.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberActivateOutcome {
    /// Freshly installed this run.
    Installed,
    /// Already installed everywhere targeted — an idempotent no-op repair.
    AlreadyInstalled,
    /// Failed to install on every target platform.
    Failed(String),
    /// The member's pinned source is missing or no longer registered.
    Unresolvable(String),
}

#[derive(Debug)]
pub struct MemberActivateStatus {
    pub kind: ArtifactKind,
    pub name: String,
    pub outcome: MemberActivateOutcome,
}

#[derive(Debug)]
pub struct SetActivateResult {
    pub name: String,
    pub members: Vec<MemberActivateStatus>,
    /// True when any member was unresolvable or failed to install everywhere.
    pub any_failed: bool,
    /// True when this was a `--dry-run` preview — no disk or state changes were made.
    pub dry_run: bool,
}

/// Per-member outcome of a `deactivate` (or `deactivate --dry-run`) run.
#[derive(Debug, PartialEq, Eq)]
pub enum MemberDeactivateOutcome {
    /// Physically uninstalled this run.
    Uninstalled,
    /// Not installed anywhere in scope — nothing to do.
    NotInstalled,
    /// Left installed because another active set still claims it.
    Retained(String),
    /// Left installed because it has local edits and `--force` wasn't passed.
    DriftBlocked,
}

#[derive(Debug)]
pub struct MemberDeactivateStatus {
    pub kind: ArtifactKind,
    pub name: String,
    pub outcome: MemberDeactivateOutcome,
}

#[derive(Debug)]
pub struct SetDeactivateResult {
    pub name: String,
    pub members: Vec<MemberDeactivateStatus>,
    /// True when a drift-blocked member (no `--force`) prevented a full deactivation.
    pub any_blocked: bool,
    /// True when this was a `--dry-run` preview — no disk or state changes were made.
    pub dry_run: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn create(
    name: &str,
    description: Option<&str>,
    from: Option<&str>,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetCreateResult> {
    // Resolve-then-mutate: fail fast, before writing anything, if `--from`
    // can't be resolved to a known source/plugin.
    let members = match from {
        Some(spec) => seed_from_plugin(spec, ctx)?,
        None => Vec::new(),
    };
    let member_count = members.len();

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        if sets.sets.contains_key(name) {
            bail!("Set '{name}' already exists.");
        }
        sets.sets.insert(
            name.to_string(),
            SetDef {
                description: description.map(str::to_string),
                state: SetState::Inactive,
                members,
            },
        );
        Ok(())
    })?;
    Ok(SetCreateResult {
        name: name.to_string(),
        member_count,
        seeded_from: from.map(str::to_string),
    })
}

/// Resolve a `<source>:<plugin>` spec to the plugin's declared `agents`/
/// `skills`, pinning each resulting member's `source` to the given source
/// name (see `SETS.md`, "Relationship to the existing 'plugin' concept").
/// Unlike [`resolve_member`], which resolves from the *lockfile* (the
/// artifact must already be installed), this resolves from the *source's*
/// marketplace — the whole point of seeding is that members need not be
/// installed yet.
fn seed_from_plugin(spec: &str, ctx: &AppContext<'_>) -> Result<Vec<SetMember>> {
    let Some((source_name, plugin_name)) = spec.split_once(':') else {
        bail!("Invalid --from value '{spec}'; expected <source>:<plugin>.");
    };
    if source_name.is_empty() || plugin_name.is_empty() {
        bail!("Invalid --from value '{spec}'; expected <source>:<plugin>.");
    }

    let sources = config::load_sources(ctx.fs, ctx.paths)?;
    let entry = sources.get_source(source_name)?;
    let root = config::resolve_local_path(entry)?;

    let marketplace_path = root.join(".claude-plugin").join("marketplace.json");
    if !ctx.fs.exists(&marketplace_path) {
        bail!(
            "Source '{source_name}' has no marketplace (.claude-plugin/marketplace.json not found)."
        );
    }

    let mut warnings = Vec::new();
    let scan =
        scan_marketplace_plugin(&root, &marketplace_path, plugin_name, ctx.fs, &mut warnings)?;

    match scan {
        PluginScan::NotFound => {
            bail!("Plugin '{plugin_name}' not found in marketplace of source '{source_name}'.");
        }
        PluginScan::RemoteUnsupported(source_type) => {
            bail!(
                "Plugin '{plugin_name}' uses remote source '{source_type}' which is not yet \
                 supported; cannot seed a set from it."
            );
        }
        PluginScan::Found(artifacts) => Ok(artifacts
            .into_iter()
            .map(|a| SetMember {
                kind: a.kind,
                name: a.name,
                source: Some(source_name.to_string()),
            })
            .collect()),
    }
}

pub fn list(scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetListResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let entries = sets
        .sets
        .into_iter()
        .map(|(name, def)| {
            let footprint_chars =
                def.members.iter().map(|m| member_description_chars(m, ctx).unwrap_or(0)).sum();
            SetListEntry {
                name,
                state: def.state,
                member_count: def.members.len(),
                footprint_chars,
            }
        })
        .collect();
    Ok(SetListResult { entries })
}

pub fn show(name: &str, scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetShowResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets.sets.get(name).ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;

    let members: Vec<SetMemberStatus> = def
        .members
        .iter()
        .map(|m| {
            let installed = InstallScope::ALL
                .iter()
                .any(|&s| ctx.paths.is_installed(m.kind, &m.name, s, ctx.fs));
            SetMemberStatus {
                kind: m.kind,
                name: m.name.clone(),
                source: m.source.clone(),
                installed,
                footprint_chars: member_description_chars(m, ctx),
            }
        })
        .collect();
    let footprint_chars = members.iter().filter_map(|m| m.footprint_chars).sum();

    Ok(SetShowResult {
        name: name.to_string(),
        description: def.description.clone(),
        state: def.state,
        members,
        footprint_chars,
    })
}

pub fn add(
    name: &str,
    artifacts: &[String],
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetAddResult> {
    // Resolve-then-mutate: fail fast, before writing anything, if any artifact
    // cannot be resolved to a kind + source.
    let resolved: Vec<SetMember> =
        artifacts.iter().map(|arg| resolve_member(arg, ctx)).collect::<Result<_>>()?;

    let mut added = Vec::new();
    let mut already = Vec::new();

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        let def = sets
            .sets
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;

        for member in resolved {
            let is_duplicate = def
                .members
                .iter()
                .any(|existing| existing.kind == member.kind && existing.name == member.name);
            if is_duplicate {
                already.push(member.name);
            } else {
                added.push(member.name.clone());
                def.members.push(member);
            }
        }
        Ok(())
    })?;

    Ok(SetAddResult {
        set: name.to_string(),
        added,
        already,
    })
}

pub fn remove(
    name: &str,
    artifacts: &[String],
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetRemoveResult> {
    let parsed: Vec<(Option<ArtifactKind>, &str)> =
        artifacts.iter().map(|a| parse_prefix(a)).collect();

    let mut removed = Vec::new();
    let mut not_found = Vec::new();

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        let def = sets
            .sets
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;

        for (hint, artifact_name) in &parsed {
            let before = def.members.len();
            def.members.retain(|m| {
                let matches = m.name == *artifact_name && hint.is_none_or(|k| m.kind == k);
                !matches
            });
            if def.members.len() < before {
                removed.push((*artifact_name).to_string());
            } else {
                not_found.push((*artifact_name).to_string());
            }
        }
        Ok(())
    })?;

    Ok(SetRemoveResult {
        set: name.to_string(),
        removed,
        not_found,
    })
}

/// Delete a set's definition. With `purge`, deactivate it first (honouring
/// reference-counting and the drift guard — `force` forwards to that
/// deactivation) so members not held by another active set are uninstalled
/// before the definition disappears. A drift-blocked member aborts the purge
/// entirely, leaving the definition (and set state) untouched so the user can
/// retry with `--force` or resolve the edits manually.
pub fn delete(
    name: &str,
    purge: bool,
    force: bool,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetDeleteResult> {
    if purge {
        let outcome = deactivate(name, force, false, scope, ctx)?;
        if outcome.any_blocked {
            bail!(
                "Cannot purge set '{name}': some members have local edits. \
                 Pass --force to discard them, or resolve the edits first."
            );
        }
    }

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        sets.sets
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;
        Ok(())
    })?;
    Ok(SetDeleteResult {
        name: name.to_string(),
    })
}

/// Install every member of `name` from its pinned source, into the normally
/// resolved install targets (a set does not pin platforms). Best-effort: a
/// member whose pinned source is no longer registered is reported
/// unresolvable and the rest still proceed; a member that fails to install on
/// every target platform is reported failed. Idempotent — already-installed
/// members are harmless no-ops (`install`'s own `decide_install`), so
/// re-running `activate` doubles as "repair this set back to fully
/// installed."
///
/// The set's state is persisted as `Active` once at least one resolvable
/// member installed (or immediately, for an empty set) — never on a run where
/// every member was unresolvable/failed. `--dry-run` performs no install
/// calls and no state write; it only classifies what would happen.
pub fn activate(
    name: &str,
    dry_run: bool,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetActivateResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets.sets.get(name).ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;
    let sources = config::load_sources(ctx.fs, ctx.paths)?;

    let mut statuses = Vec::new();
    let mut resolvable: Vec<SetMember> = Vec::new();
    for m in &def.members {
        match &m.source {
            Some(src) if sources.sources.contains_key(src) => resolvable.push(m.clone()),
            Some(src) => statuses.push(MemberActivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberActivateOutcome::Unresolvable(format!(
                    "source '{src}' is not registered"
                )),
            }),
            None => statuses.push(MemberActivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberActivateOutcome::Unresolvable("no source pin recorded".to_string()),
            }),
        }
    }
    let members_is_empty = def.members.is_empty();

    for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
        let members: Vec<&SetMember> = resolvable.iter().filter(|m| m.kind == kind).collect();
        if members.is_empty() {
            continue;
        }

        let targets = install::resolve_targets(None, kind, scope, ctx)?;
        let pre_installed: HashSet<&str> = members
            .iter()
            .filter(|m| {
                targets
                    .iter()
                    .any(|&t| ctx.paths.with_platform(t).is_installed(kind, &m.name, scope, ctx.fs))
            })
            .map(|m| m.name.as_str())
            .collect();

        let failed: HashMap<String, String> = if dry_run {
            HashMap::new()
        } else {
            let pinned: Vec<String> = members
                .iter()
                .map(|m| format!("{}:{}", m.source.as_deref().unwrap_or_default(), m.name))
                .collect();
            let result = install::install_many(&pinned, kind, scope, false, &targets, ctx)?;
            result.failed.into_iter().collect()
        };

        for m in members {
            let pin = format!("{}:{}", m.source.as_deref().unwrap_or_default(), m.name);
            let outcome = if let Some(reason) = failed.get(&pin) {
                MemberActivateOutcome::Failed(reason.clone())
            } else if pre_installed.contains(m.name.as_str()) {
                MemberActivateOutcome::AlreadyInstalled
            } else {
                MemberActivateOutcome::Installed
            };
            statuses.push(MemberActivateStatus {
                kind,
                name: m.name.clone(),
                outcome,
            });
        }
    }

    let any_failed = statuses.iter().any(|s| {
        matches!(
            s.outcome,
            MemberActivateOutcome::Unresolvable(_) | MemberActivateOutcome::Failed(_)
        )
    });

    if !dry_run {
        let any_installed_ok = statuses.iter().any(|s| {
            matches!(
                s.outcome,
                MemberActivateOutcome::Installed | MemberActivateOutcome::AlreadyInstalled
            )
        });
        if members_is_empty || any_installed_ok {
            config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
                if let Some(d) = sets.sets.get_mut(name) {
                    d.state = SetState::Active;
                }
                Ok(())
            })?;
        }
    }

    Ok(SetActivateResult {
        name: name.to_string(),
        members: statuses,
        any_failed,
        dry_run,
    })
}

/// Uninstall every member of `name`, reference-counted against other active
/// sets and guarded against clobbering local edits.
///
/// A member still claimed by another `Active` set is retained (only the
/// claim is dropped, per `SETS.md`'s reference-counting rule). A member with
/// local edits (the same drift detection `install` uses) blocks that
/// member's uninstall unless `force` is passed. The set's state is persisted
/// as `Inactive` only when no member was drift-blocked — a partially
/// deactivated set stays `Active` so `set show`/`doctor` can surface the gap
/// (see `SETS.md`, "Drift is surfaced, not auto-corrected"). `--dry-run`
/// performs no uninstall calls and no state write.
pub fn deactivate(
    name: &str,
    force: bool,
    dry_run: bool,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetDeactivateResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets
        .sets
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?
        .clone();

    let mut statuses = Vec::new();
    let mut any_blocked = false;

    for m in &def.members {
        let (installed, drifted) = member_activation_facts(m.kind, &m.name, scope, ctx)?;
        if !installed {
            statuses.push(MemberDeactivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberDeactivateOutcome::NotInstalled,
            });
            continue;
        }
        if let Some(holder) = held_by_other_active_set(m.kind, &m.name, name, &sets) {
            statuses.push(MemberDeactivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberDeactivateOutcome::Retained(holder),
            });
            continue;
        }
        if drifted && !force {
            any_blocked = true;
            statuses.push(MemberDeactivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberDeactivateOutcome::DriftBlocked,
            });
            continue;
        }
        if !dry_run {
            uninstall::uninstall(&m.name, m.kind, scope, None, ctx)?;
        }
        statuses.push(MemberDeactivateStatus {
            kind: m.kind,
            name: m.name.clone(),
            outcome: MemberDeactivateOutcome::Uninstalled,
        });
    }

    if !dry_run && !any_blocked {
        config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
            if let Some(d) = sets.sets.get_mut(name) {
                d.state = SetState::Inactive;
            }
            Ok(())
        })?;
    }

    Ok(SetDeactivateResult {
        name: name.to_string(),
        members: statuses,
        any_blocked,
        dry_run,
    })
}

pub fn rename(
    old: &str,
    new: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetRenameResult> {
    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        if sets.sets.contains_key(new) {
            bail!("Set '{new}' already exists.");
        }
        let def = sets.sets.remove(old).ok_or_else(|| anyhow::anyhow!("Set '{old}' not found."))?;
        sets.sets.insert(new.to_string(), def);
        Ok(())
    })?;
    Ok(SetRenameResult {
        old: old.to_string(),
        new: new.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Split an optional `skill:`/`agent:` disambiguation prefix off an artifact
/// argument. `source:name` disambiguation is deferred (SETS.md Phase 4).
fn parse_prefix(arg: &str) -> (Option<ArtifactKind>, &str) {
    if let Some(rest) = arg.strip_prefix("skill:") {
        (Some(ArtifactKind::Skill), rest)
    } else if let Some(rest) = arg.strip_prefix("agent:") {
        (Some(ArtifactKind::Agent), rest)
    } else {
        (None, arg)
    }
}

/// Resolve an artifact argument to its kind and pinned source by consulting
/// the lockfile (see `SETS.md`, "The source pin"). Searches both install
/// scopes; a bare name that is ambiguous across kinds requires a `skill:`/
/// `agent:` prefix to disambiguate.
fn resolve_member(arg: &str, ctx: &AppContext<'_>) -> Result<SetMember> {
    let (hint, name) = parse_prefix(arg);

    let mut candidates: Vec<(ArtifactKind, String)> = Vec::new();
    for scope in InstallScope::ALL {
        let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
        if let Some(entry) = lock.packages.get(name) {
            candidates.push((entry.artifact_type, entry.source.repo.clone()));
        }
    }

    if candidates.is_empty() {
        bail!(
            "Artifact '{name}' is not installed (no lockfile entry); cannot resolve its kind/source."
        );
    }

    let candidates: Vec<(ArtifactKind, String)> = match hint {
        Some(k) => candidates.into_iter().filter(|(kind, _)| *kind == k).collect(),
        None => candidates,
    };

    if candidates.is_empty() {
        bail!("Artifact '{name}' has no lockfile entry matching the requested kind.");
    }

    let distinct_kinds: HashSet<ArtifactKind> = candidates.iter().map(|(k, _)| *k).collect();
    if hint.is_none() && distinct_kinds.len() > 1 {
        bail!(
            "'{name}' is ambiguous across kinds — use skill:{name} or agent:{name} to disambiguate."
        );
    }

    let (kind, source) = candidates.into_iter().next().expect("non-empty, checked above");
    Ok(SetMember {
        kind,
        name: name.to_string(),
        source: Some(source),
    })
}

/// Sweep every candidate platform (the same "managed-or-all" set `uninstall`
/// sweeps) gathering the same drift/already-installed facts `install` itself
/// uses, so `deactivate` blocks on local edits and skips already-absent
/// members exactly as install/uninstall would.
fn member_activation_facts(
    kind: ArtifactKind,
    member_name: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<(bool, bool)> {
    let candidates = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    let mut installed = false;
    let mut modified = false;
    for view in platform_iter::views_for(ctx.paths, candidates, kind) {
        let pctx = ctx.with_paths(&view.paths);
        let facts = install::gather_install_facts(member_name, kind, scope, false, &pctx)?;
        installed |= facts.already_installed;
        modified |= facts.locally_modified;
    }
    Ok((installed, modified))
}

/// Resolve a member's trigger-description character count (see `SETS.md`,
/// "Context-footprint reporting"): read the installed copy's `description`
/// frontmatter when the member happens to be installed (either scope), else
/// fall back to its pinned source. `None` when neither yields a description —
/// callers treat that as an unresolvable member, counted as 0 in totals and
/// rendered as `?`.
fn member_description_chars(m: &SetMember, ctx: &AppContext<'_>) -> Option<usize> {
    InstallScope::ALL
        .iter()
        .find_map(|&scope| installed_description_chars(m, scope, ctx))
        .or_else(|| source_description_chars(m, ctx))
}

fn installed_description_chars(
    m: &SetMember,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Option<usize> {
    let path = ctx.paths.installed_artifact_path(m.kind, &m.name, scope)?;
    if !ctx.fs.exists(&path) {
        return None;
    }
    let content = ctx.fs.read_to_string(&m.kind.content_path(&path)).ok()?;
    let (frontmatter, _) = crate::scan::split_frontmatter_and_body(&content);
    crate::scan::extract_field(&frontmatter?, "description").map(|d| d.chars().count())
}

fn source_description_chars(m: &SetMember, ctx: &AppContext<'_>) -> Option<usize> {
    source_iter::find_unique(&m.name, m.kind, m.source.as_deref(), ctx)
        .ok()
        .map(|sa| sa.artifact.description.chars().count())
}

/// The name of another `Active` set that still claims `(kind, member_name)`,
/// if any — the reference-counting check that keeps `deactivate` from
/// uninstalling a member a sibling set still needs (see `SETS.md`, "Lifecycle
/// semantics").
fn held_by_other_active_set(
    kind: ArtifactKind,
    member_name: &str,
    this_set: &str,
    sets: &SetsFile,
) -> Option<String> {
    sets.sets.iter().find_map(|(set_name, other_def)| {
        if set_name == this_set || other_def.state != SetState::Active {
            return None;
        }
        other_def
            .members
            .iter()
            .any(|m| m.kind == kind && m.name == member_name)
            .then(|| set_name.clone())
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestContext, make_lock_entry_builder, save_lock_with_entry};

    // --- create ---

    #[test]
    fn create_errors_when_name_exists() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result = create("rust-work", None, None, InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn create_inserts_inactive_empty_set() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("blog", Some("blog work"), None, InstallScope::Global, &ctx).unwrap();

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("blog").unwrap();
        assert_eq!(def.state, SetState::Inactive);
        assert!(def.members.is_empty());
        assert_eq!(def.description.as_deref(), Some("blog work"));
    }

    // --- create --from (plugin seeding) ---

    fn marketplace_json(plugins_json: &str) -> String {
        format!(r#"{{"name":"test","plugins":[{plugins_json}]}}"#)
    }

    #[test]
    fn create_from_seeds_members_inactive() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file(
            "/src/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"my-plugin","agents":["./agents/my-agent.md"],"skills":["./skills/my-skill"]}"#,
            ),
        );
        t.fs.add_file(
            "/src/agents/my-agent.md",
            crate::test_support::agent_content("my-agent", "An agent"),
        );
        t.fs.add_file(
            "/src/skills/my-skill/SKILL.md",
            crate::test_support::skill_content("A skill"),
        );

        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:my-plugin"), InstallScope::Global, &ctx)
                .unwrap();
        assert_eq!(result.member_count, 2);
        assert_eq!(result.seeded_from.as_deref(), Some("guidelines:my-plugin"));

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("rust-work").unwrap();
        assert_eq!(def.state, SetState::Inactive);
        assert_eq!(def.members.len(), 2);
        assert!(def.members.iter().all(|m| m.source.as_deref() == Some("guidelines")));
        let kinds: HashSet<ArtifactKind> = def.members.iter().map(|m| m.kind).collect();
        assert!(kinds.contains(&ArtifactKind::Agent));
        assert!(kinds.contains(&ArtifactKind::Skill));
    }

    #[test]
    fn create_from_unknown_plugin_errors_and_creates_nothing() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file(
            "/src/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","agents":["./agents/my-agent.md"]}"#),
        );

        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:ghost"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn create_from_remote_plugin_reports_unsupported() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file(
            "/src/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"remote-plugin","source":{"source":"url","url":"https://example.com"}}"#,
            ),
        );

        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:remote-plugin"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not yet supported"));

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn create_from_unknown_source_errors_and_creates_nothing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("ghost-source:my-plugin"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn create_from_source_without_marketplace_errors() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:my-plugin"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no marketplace"));
    }

    #[test]
    fn create_from_malformed_spec_errors() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = create("rust-work", None, Some("no-colon-here"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected <source>:<plugin>"));
    }

    // --- list ---

    #[test]
    fn list_reports_state_and_member_count() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();

        let result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "rust-work");
        assert_eq!(result.entries[0].state, SetState::Inactive);
        assert_eq!(result.entries[0].member_count, 0);
        assert_eq!(result.entries[0].footprint_chars, 0);
    }

    // --- footprint ---

    #[test]
    fn list_footprint_reads_installed_members_description() {
        let t = TestContext::new();
        crate::test_support::install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &crate::test_support::agent_content("my-agent", "Some description"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("my-agent", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        let result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.entries[0].footprint_chars, "Some description".chars().count());
    }

    #[test]
    fn show_footprint_for_inactive_member_resolves_from_source() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        let result = show("rust-work", InstallScope::Global, &ctx).unwrap();
        let expected = "A test agent".chars().count();
        assert_eq!(result.members[0].footprint_chars, Some(expected));
        assert_eq!(result.footprint_chars, expected);
    }

    #[test]
    fn footprint_unresolvable_member_counts_as_zero_and_none() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("ghost", "gone-source")],
            InstallScope::Global,
            &ctx,
        );

        let list_result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(list_result.entries[0].footprint_chars, 0);

        let show_result = show("rust-work", InstallScope::Global, &ctx).unwrap();
        assert_eq!(show_result.members[0].footprint_chars, None);
        assert_eq!(show_result.footprint_chars, 0);
    }

    // --- show ---

    #[test]
    fn show_errors_when_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = show("nope", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    // --- add: resolves source from lockfile ---

    #[test]
    fn add_resolves_source_from_lockfile() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "rust-craftsperson",
            make_lock_entry_builder(
                ArtifactKind::Agent,
                "guidelines",
                "agents/rust-craftsperson.md",
            ),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result =
            add("rust-work", &["rust-craftsperson".to_string()], InstallScope::Global, &ctx)
                .unwrap();
        assert_eq!(result.added, vec!["rust-craftsperson".to_string()]);
        assert!(result.already.is_empty());

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("rust-work").unwrap();
        assert_eq!(def.members.len(), 1);
        assert_eq!(def.members[0].kind, ArtifactKind::Agent);
        assert_eq!(def.members[0].source.as_deref(), Some("guidelines"));
    }

    #[test]
    fn add_errors_when_artifact_not_installed() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result = add("rust-work", &["ghost".to_string()], InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn add_does_not_write_when_any_artifact_unresolvable() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result = add(
            "rust-work",
            &["known".to_string(), "ghost".to_string()],
            InstallScope::Global,
            &ctx,
        );
        assert!(result.is_err());

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(sets.sets.get("rust-work").unwrap().members.is_empty());
    }

    #[test]
    fn add_duplicate_is_noop() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        add("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();
        let result = add("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();
        assert!(result.added.is_empty());
        assert_eq!(result.already, vec!["known".to_string()]);

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().members.len(), 1);
    }

    #[test]
    fn add_errors_when_set_missing() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        let result = add("nope", &["known".to_string()], InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn resolve_member_ambiguous_across_kinds_errors() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "dual",
            make_lock_entry_builder(ArtifactKind::Agent, "guidelines", "agents/dual.md"),
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "dual",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "dual"),
            InstallScope::Local,
        );

        let ctx = t.ctx();
        create("mixed", None, None, InstallScope::Global, &ctx).unwrap();

        let bare = add("mixed", &["dual".to_string()], InstallScope::Global, &ctx);
        assert!(bare.is_err());
        assert!(bare.unwrap_err().to_string().contains("ambiguous"));

        let prefixed =
            add("mixed", &["skill:dual".to_string()], InstallScope::Global, &ctx).unwrap();
        assert_eq!(prefixed.added, vec!["dual".to_string()]);
    }

    // --- remove ---

    #[test]
    fn remove_drops_member_without_uninstalling() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        add("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();

        let result =
            remove("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.removed, vec!["known".to_string()]);
        assert!(result.not_found.is_empty());

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(sets.sets.get("rust-work").unwrap().members.is_empty());
        // Still tracked in the lockfile — remove does not uninstall.
        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(lock.packages.contains_key("known"));
    }

    #[test]
    fn remove_reports_not_found_for_missing_member() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result =
            remove("rust-work", &["ghost".to_string()], InstallScope::Global, &ctx).unwrap();
        assert!(result.removed.is_empty());
        assert_eq!(result.not_found, vec!["ghost".to_string()]);
    }

    // --- delete ---

    #[test]
    fn delete_removes_definition() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        delete("rust-work", false, false, InstallScope::Global, &ctx).unwrap();
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn delete_errors_when_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = delete("nope", false, false, InstallScope::Global, &ctx);
        assert!(result.is_err());
    }

    // --- rename ---

    #[test]
    fn rename_errors_when_old_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = rename("nope", "new", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn rename_errors_when_new_exists() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("old", None, None, InstallScope::Global, &ctx).unwrap();
        create("new", None, None, InstallScope::Global, &ctx).unwrap();
        let result = rename("old", "new", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn rename_moves_definition_to_new_key() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("old", Some("desc"), None, InstallScope::Global, &ctx).unwrap();
        rename("old", "new", InstallScope::Global, &ctx).unwrap();

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("old"));
        let def = sets.sets.get("new").unwrap();
        assert_eq!(def.description.as_deref(), Some("desc"));
    }

    // --- activate / deactivate ---

    /// Directly seed a set's members (bypassing `add`'s lockfile-resolution
    /// requirement) — Phase 2 tests care about the pinned source, not how the
    /// member first got there.
    fn seed_members(
        set_name: &str,
        members: Vec<SetMember>,
        scope: InstallScope,
        ctx: &AppContext<'_>,
    ) {
        config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
            sets.sets.get_mut(set_name).unwrap().members = members;
            Ok(())
        })
        .unwrap();
    }

    fn pinned_agent(name: &str, source: &str) -> SetMember {
        SetMember {
            kind: ArtifactKind::Agent,
            name: name.to_string(),
            source: Some(source.to_string()),
        }
    }

    fn pinned_skill(name: &str, source: &str) -> SetMember {
        SetMember {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            source: Some(source.to_string()),
        }
    }

    #[test]
    fn activate_installs_all_members_and_sets_state_active() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        crate::test_support::setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "foundry",
            "1.0.0",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![
                pinned_agent("rust-craftsperson", "guidelines"),
                pinned_skill("foundry", "guidelines"),
            ],
            InstallScope::Global,
            &ctx,
        );

        let result = activate("rust-work", false, InstallScope::Global, &ctx).unwrap();
        assert!(!result.any_failed);
        assert!(t.paths.is_installed(
            ArtifactKind::Agent,
            "rust-craftsperson",
            InstallScope::Global,
            &t.fs
        ));
        assert!(
            t.paths
                .is_installed(ArtifactKind::Skill, "foundry", InstallScope::Global, &t.fs)
        );

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Active);
    }

    #[test]
    fn activate_is_idempotent_on_already_installed_members() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();
        let second = activate("rust-work", false, InstallScope::Global, &ctx).unwrap();

        assert!(!second.any_failed);
        assert!(matches!(second.members[0].outcome, MemberActivateOutcome::AlreadyInstalled));
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Active);
    }

    #[test]
    fn activate_reports_unresolvable_source_installs_rest_and_fails() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![
                pinned_agent("rust-craftsperson", "guidelines"),
                pinned_skill("ghost-skill", "gone-source"),
            ],
            InstallScope::Global,
            &ctx,
        );

        let result = activate("rust-work", false, InstallScope::Global, &ctx).unwrap();
        assert!(result.any_failed);
        assert!(
            t.paths.is_installed(
                ArtifactKind::Agent,
                "rust-craftsperson",
                InstallScope::Global,
                &t.fs
            ),
            "the resolvable member still installs"
        );
        let ghost = result.members.iter().find(|m| m.name == "ghost-skill").unwrap();
        assert!(matches!(ghost.outcome, MemberActivateOutcome::Unresolvable(_)));

        // At least one resolvable member installed, so the set is still marked Active.
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Active);
    }

    #[test]
    fn activate_dry_run_makes_no_changes() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        let result = activate("rust-work", true, InstallScope::Global, &ctx).unwrap();
        assert!(result.dry_run);
        assert!(
            !t.paths.is_installed(
                ArtifactKind::Agent,
                "rust-craftsperson",
                InstallScope::Global,
                &t.fs
            ),
            "dry-run must not install anything"
        );
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Inactive);
    }

    #[test]
    fn deactivate_uninstalls_members_and_sets_state_inactive() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();

        let result = deactivate("rust-work", false, false, InstallScope::Global, &ctx).unwrap();
        assert!(!result.any_blocked);
        assert!(matches!(result.members[0].outcome, MemberDeactivateOutcome::Uninstalled));
        assert!(!t.paths.is_installed(
            ArtifactKind::Agent,
            "rust-craftsperson",
            InstallScope::Global,
            &t.fs
        ));
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Inactive);
    }

    #[test]
    fn deactivate_retains_member_held_by_another_active_set() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_skill(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "foundry",
            "1.0.0",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        create("blog", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_skill("foundry", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        seed_members(
            "blog",
            vec![pinned_skill("foundry", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();
        activate("blog", false, InstallScope::Global, &ctx).unwrap();

        let result = deactivate("rust-work", false, false, InstallScope::Global, &ctx).unwrap();
        assert!(!result.any_blocked);
        assert!(matches!(
            result.members[0].outcome,
            MemberDeactivateOutcome::Retained(ref holder) if holder == "blog"
        ));
        assert!(
            t.paths
                .is_installed(ArtifactKind::Skill, "foundry", InstallScope::Global, &t.fs),
            "still held by 'blog', must not be uninstalled"
        );
    }

    #[test]
    fn deactivate_drift_guard_blocks_without_force_and_proceeds_with_force() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();

        // Simulate a local hand-edit of the installed copy.
        let installed_path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "rust-craftsperson", InstallScope::Global)
            .unwrap();
        t.fs.add_file(
            installed_path.clone(),
            "---\nname: rust-craftsperson\n---\nedited by hand\n",
        );

        let blocked = deactivate("rust-work", false, false, InstallScope::Global, &ctx).unwrap();
        assert!(blocked.any_blocked);
        assert!(matches!(blocked.members[0].outcome, MemberDeactivateOutcome::DriftBlocked));
        assert!(t.fs.file_exists(&installed_path), "drifted copy must be left in place");
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(
            sets.sets.get("rust-work").unwrap().state,
            SetState::Active,
            "partial deactivation leaves the set Active"
        );

        let forced = deactivate("rust-work", true, false, InstallScope::Global, &ctx).unwrap();
        assert!(!forced.any_blocked);
        assert!(matches!(forced.members[0].outcome, MemberDeactivateOutcome::Uninstalled));
        assert!(!t.fs.file_exists(&installed_path), "--force discards the edits and uninstalls");
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Inactive);
    }

    #[test]
    fn deactivate_dry_run_makes_no_changes() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();

        let result = deactivate("rust-work", false, true, InstallScope::Global, &ctx).unwrap();
        assert!(result.dry_run);
        assert!(
            t.paths.is_installed(
                ArtifactKind::Agent,
                "rust-craftsperson",
                InstallScope::Global,
                &t.fs
            ),
            "dry-run must not uninstall anything"
        );
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().state, SetState::Active);
    }

    #[test]
    fn delete_purge_deactivates_then_deletes() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();

        delete("rust-work", true, false, InstallScope::Global, &ctx).unwrap();

        assert!(!t.paths.is_installed(
            ArtifactKind::Agent,
            "rust-craftsperson",
            InstallScope::Global,
            &t.fs
        ));
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn delete_purge_with_drift_blocked_member_preserves_definition() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", false, InstallScope::Global, &ctx).unwrap();
        let installed_path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "rust-craftsperson", InstallScope::Global)
            .unwrap();
        t.fs.add_file(installed_path.clone(), "edited by hand");

        let result = delete("rust-work", true, false, InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(t.fs.file_exists(&installed_path));
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(
            sets.sets.contains_key("rust-work"),
            "definition preserved when purge is blocked"
        );

        // --force forwards through and completes the purge.
        delete("rust-work", true, true, InstallScope::Global, &ctx).unwrap();
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }
}
