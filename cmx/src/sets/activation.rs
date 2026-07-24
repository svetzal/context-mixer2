//! Set activation and deactivation logic.

use crate::error::{CliError, Result};
use crate::flags::{Force, RunMode};
use std::collections::{HashMap, HashSet};

use crate::config;
use crate::context::AppContext;
use crate::diff::file_changes_between;
use crate::install;
use crate::platform::Platform;
use crate::platform_copies::gather_platform_copies;
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope, SetMember, SetState, SetsFile, SourcesFile};
use crate::uninstall;

use super::{
    MemberActivateOutcome, MemberActivateStatus, MemberActivateTarget, MemberDeactivateOutcome,
    MemberDeactivateStatus, MemberDeactivateTarget, SetActivateResult, SetDeactivateResult,
};

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
/// every member was unresolvable/failed. Without `--apply`, no install calls
/// or state writes occur; the command only describes the concrete plan.
pub fn activate(
    name: &str,
    mode: RunMode,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetActivateResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets.sets.get(name).ok_or_else(|| CliError::SetNotFound {
        name: name.to_string(),
    })?;
    let sources = config::load_sources(ctx.fs, ctx.paths)?;

    let members_is_empty = def.members.is_empty();
    let (resolvable, mut statuses) = partition_members(&def.members, &sources);

    for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
        statuses.extend(activate_kind(kind, &resolvable, scope, mode, ctx)?);
    }

    let any_failed = statuses.iter().any(|s| {
        matches!(
            s.outcome,
            MemberActivateOutcome::Unresolvable(_) | MemberActivateOutcome::Failed(_)
        )
    });

    persist_active_state(name, members_is_empty, &statuses, mode, scope, ctx)?;

    Ok(SetActivateResult {
        name: name.to_string(),
        members: statuses,
        any_failed,
        apply: mode.is_apply(),
    })
}

/// Partition `members` into resolvable (source registered) and unresolvable
/// (missing source pin or unregistered source), returning the statuses for the
/// unresolvable ones alongside the resolvable member list.
fn partition_members(
    members: &[SetMember],
    sources: &SourcesFile,
) -> (Vec<SetMember>, Vec<MemberActivateStatus>) {
    let mut statuses = Vec::new();
    let mut resolvable = Vec::new();
    for m in members {
        match &m.source {
            Some(src) if sources.sources.contains_key(src) => resolvable.push(m.clone()),
            Some(src) => statuses.push(MemberActivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberActivateOutcome::Unresolvable(format!(
                    "source '{src}' is not registered"
                )),
                targets: Vec::new(),
            }),
            None => statuses.push(MemberActivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberActivateOutcome::Unresolvable("no source pin recorded".to_string()),
                targets: Vec::new(),
            }),
        }
    }
    (resolvable, statuses)
}

/// Install all `resolvable` members of the given `kind`, returning their
/// per-member statuses. Returns an empty vec when no members of this kind
/// exist in `resolvable`.
fn activate_kind(
    kind: ArtifactKind,
    resolvable: &[SetMember],
    scope: InstallScope,
    mode: RunMode,
    ctx: &AppContext<'_>,
) -> Result<Vec<MemberActivateStatus>> {
    let members: Vec<&SetMember> = resolvable.iter().filter(|m| m.kind == kind).collect();
    if members.is_empty() {
        return Ok(Vec::new());
    }

    let install_targets = install::resolve_targets(None, kind, scope, ctx)?;
    let pre_installed: HashSet<&str> = members
        .iter()
        .filter(|m| {
            install_targets
                .iter()
                .any(|&t| ctx.paths.with_platform(t).is_installed(kind, &m.name, scope, ctx.fs))
        })
        .map(|m| m.name.as_str())
        .collect();

    let failed: HashMap<String, String> = if mode.is_apply() {
        let pinned: Vec<String> = members
            .iter()
            .map(|m| format!("{}:{}", m.source.as_deref().unwrap_or_default(), m.name))
            .collect();
        let result = install::install_many(&pinned, kind, scope, Force::No, &install_targets, ctx)?;
        result.failed.into_iter().collect()
    } else {
        HashMap::new()
    };

    let mut statuses = Vec::new();
    for m in members {
        let member_targets = build_activation_targets(m, scope, &install_targets, ctx)?;
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
            targets: member_targets,
        });
    }
    Ok(statuses)
}

/// Persist the `Active` state for the set when `mode` is [`RunMode::Apply`]
/// and at least one member installed successfully (or the set is empty).
fn persist_active_state(
    name: &str,
    members_is_empty: bool,
    statuses: &[MemberActivateStatus],
    mode: RunMode,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<()> {
    if !mode.is_apply() {
        return Ok(());
    }
    let any_installed_ok = statuses.iter().any(|s| {
        matches!(
            s.outcome,
            MemberActivateOutcome::Installed | MemberActivateOutcome::AlreadyInstalled
        )
    });
    if members_is_empty || any_installed_ok {
        config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
            if let Some(d) = sets.sets.get_mut(name) {
                d.state = SetState::Active;
            }
            Ok(())
        })?;
    }
    Ok(())
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
/// (see `SETS.md`, "Drift is surfaced, not auto-corrected"). Without
/// `--apply`, no uninstall calls or state writes occur.
pub fn deactivate(
    name: &str,
    force: Force,
    mode: RunMode,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetDeactivateResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets
        .sets
        .get(name)
        .ok_or_else(|| CliError::SetNotFound {
            name: name.to_string(),
        })?
        .clone();

    let mut statuses = Vec::new();
    let mut any_blocked = false;

    for m in &def.members {
        let targets = member_deactivate_targets(m, scope, ctx)?;
        let installed = !targets.is_empty();
        let drifted = targets.iter().any(|target| !target.discarded_paths.is_empty());
        if !installed {
            statuses.push(MemberDeactivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberDeactivateOutcome::NotInstalled,
                targets,
            });
            continue;
        }
        if let Some(holder) = held_by_other_active_set(m.kind, &m.name, name, &sets) {
            statuses.push(MemberDeactivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberDeactivateOutcome::Retained(holder),
                targets,
            });
            continue;
        }
        if drifted && !force.is_yes() {
            any_blocked = true;
            statuses.push(MemberDeactivateStatus {
                kind: m.kind,
                name: m.name.clone(),
                outcome: MemberDeactivateOutcome::DriftBlocked,
                targets,
            });
            continue;
        }
        if mode.is_apply() {
            uninstall::uninstall(&m.name, m.kind, scope, None, ctx)?;
        }
        statuses.push(MemberDeactivateStatus {
            kind: m.kind,
            name: m.name.clone(),
            outcome: MemberDeactivateOutcome::Uninstalled,
            targets,
        });
    }

    if mode.is_apply() && !any_blocked {
        config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
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
        apply: mode.is_apply(),
    })
}

fn build_activation_targets(
    member: &SetMember,
    scope: InstallScope,
    targets: &[Platform],
    ctx: &AppContext<'_>,
) -> Result<Vec<MemberActivateTarget>> {
    let found = source_iter::find_unique(&member.name, member.kind, member.source.as_deref(), ctx)?;
    let mut plans = Vec::new();
    for &platform in targets {
        let pv = ctx.paths.with_platform(platform);
        let plan = install::plan_install(&member.name, member.kind, scope, &found, &pv)?;
        let target_path =
            member
                .kind
                .installed_path(&member.name, &plan.dest_dir, ArtifactKind::HOME_AGENT_EXT);
        plans.push(MemberActivateTarget {
            platform,
            source_path: found.artifact.path.clone(),
            target_path,
            version: found.artifact.version.clone(),
        });
    }
    Ok(plans)
}

fn member_deactivate_targets(
    member: &SetMember,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<MemberDeactivateTarget>> {
    let candidates = config::managed_or_all_platforms(ctx.fs, ctx.paths)?;
    let source_artifact = member.source.as_deref().and_then(|source| {
        source_iter::find_unique(&member.name, member.kind, Some(source), ctx).ok()
    });
    // Use the first platform in each shared-dir group to probe install facts;
    // platforms sharing a dir share the same physical files, so one probe suffices.
    gather_platform_copies(
        &candidates,
        member.kind,
        &member.name,
        scope,
        ctx,
        |artifact_path, platforms| {
            let platform_paths = ctx.paths.with_platform(platforms[0]);
            let pctx = ctx.with_paths(&platform_paths);
            let facts =
                install::gather_install_facts(&member.name, member.kind, scope, Force::No, &pctx)?;
            if !facts.already_installed {
                return Ok(None);
            }
            let discarded_paths = if facts.locally_modified {
                if let Some(source_artifact) = &source_artifact {
                    file_changes_between(
                        member.kind,
                        &artifact_path,
                        &source_artifact.artifact.path,
                        &pctx,
                    )?
                    .into_iter()
                    .map(|change| {
                        if artifact_path.is_file() {
                            artifact_path.clone()
                        } else {
                            artifact_path.join(change.path)
                        }
                    })
                    .collect()
                } else {
                    vec![artifact_path.clone()]
                }
            } else {
                Vec::new()
            };
            Ok(Some(MemberDeactivateTarget {
                platforms,
                artifact_path,
                discarded_paths,
            }))
        },
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::flags::{Force, RunMode};
    use crate::test_support::TestContext;
    use crate::types::{ArtifactKind, InstallScope, SetMember, SetState};

    fn seed_members(
        set_name: &str,
        members: Vec<SetMember>,
        scope: InstallScope,
        ctx: &AppContext<'_>,
    ) {
        config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
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
    fn partition_members_separates_resolvable_from_unresolvable() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        let sources = crate::config::load_sources(ctx.fs, ctx.paths).unwrap();

        let members = vec![
            SetMember {
                kind: ArtifactKind::Agent,
                name: "rust-craftsperson".to_string(),
                source: Some("guidelines".to_string()),
            },
            SetMember {
                kind: ArtifactKind::Skill,
                name: "ghost-skill".to_string(),
                source: Some("gone-source".to_string()),
            },
            SetMember {
                kind: ArtifactKind::Skill,
                name: "no-pin".to_string(),
                source: None,
            },
        ];

        let (resolvable, unresolvable) = partition_members(&members, &sources);
        assert_eq!(resolvable.len(), 1, "only the registered-source member is resolvable");
        assert_eq!(resolvable[0].name, "rust-craftsperson");
        assert_eq!(unresolvable.len(), 2, "unregistered-source and no-pin both unresolvable");
        assert!(matches!(unresolvable[0].outcome, MemberActivateOutcome::Unresolvable(_)));
        assert!(matches!(unresolvable[1].outcome, MemberActivateOutcome::Unresolvable(_)));
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![
                pinned_agent("rust-craftsperson", "guidelines"),
                pinned_skill("foundry", "guidelines"),
            ],
            InstallScope::Global,
            &ctx,
        );

        let result = activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();
        let second = activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();

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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![
                pinned_agent("rust-craftsperson", "guidelines"),
                pinned_skill("ghost-skill", "gone-source"),
            ],
            InstallScope::Global,
            &ctx,
        );

        let result = activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        let result = activate("rust-work", RunMode::Plan, InstallScope::Global, &ctx).unwrap();
        assert!(!result.apply);
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();

        let result =
            deactivate("rust-work", Force::No, RunMode::Apply, InstallScope::Global, &ctx).unwrap();
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        crate::sets::create("blog", None, None, InstallScope::Global, &ctx).unwrap();
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
        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();
        activate("blog", RunMode::Apply, InstallScope::Global, &ctx).unwrap();

        let result =
            deactivate("rust-work", Force::No, RunMode::Apply, InstallScope::Global, &ctx).unwrap();
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();

        // Simulate a local hand-edit of the installed copy.
        let installed_path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "rust-craftsperson", InstallScope::Global)
            .unwrap();
        t.fs.add_file(
            installed_path.clone(),
            "---\nname: rust-craftsperson\n---\nedited by hand\n",
        );

        let blocked =
            deactivate("rust-work", Force::No, RunMode::Apply, InstallScope::Global, &ctx).unwrap();
        assert!(blocked.any_blocked);
        assert!(matches!(blocked.members[0].outcome, MemberDeactivateOutcome::DriftBlocked));
        assert!(t.fs.file_exists(&installed_path), "drifted copy must be left in place");
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(
            sets.sets.get("rust-work").unwrap().state,
            SetState::Active,
            "partial deactivation leaves the set Active"
        );

        let forced =
            deactivate("rust-work", Force::Yes, RunMode::Apply, InstallScope::Global, &ctx)
                .unwrap();
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
        crate::sets::create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();

        let result =
            deactivate("rust-work", Force::No, RunMode::Plan, InstallScope::Global, &ctx).unwrap();
        assert!(!result.apply);
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
}
