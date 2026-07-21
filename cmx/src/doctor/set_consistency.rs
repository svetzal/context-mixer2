//! Set-consistency check for `cmx doctor` — Phase 3 of the sets feature (see
//! `SETS.md`, "doctor integration").
//!
//! Pure and read-only: given a scope's [`SetsFile`] and a predicate answering
//! "is this artifact installed", flags two kinds of drift between declared set
//! state and what's actually on disk:
//!
//! - An **active** set's member that isn't installed (`ActiveMissing`).
//! - An **inactive** set's member that's still installed *and* not claimed by
//!   any other active set (`InactiveLingering`) — mirroring the same
//!   reference-counting rule `deactivate` uses, so a member two sets share is
//!   never flagged while either set is active.

use std::collections::HashSet;

use serde::Serialize;

use crate::config;
use crate::context::AppContext;
use crate::error::Result;
use crate::types::{ArtifactKind, InstallScope, SetState, SetsFile};

use super::types::DoctorArtifact;

/// The kind of set/installed-state mismatch found.
///
/// Derives `Serialize` with `snake_case` so `doctor --json` emits
/// `"active_missing"` / `"inactive_lingering"` without a parallel string
/// mapping — one home for the machine-readable label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetProblem {
    /// The set is `Active` but this member is not installed.
    ActiveMissing,
    /// The set is `Inactive` but this member is installed and not held by any
    /// other `Active` set.
    InactiveLingering,
}

/// One set/member mismatch surfaced by the consistency check.
///
/// Derives `Serialize` so `doctor --json` can emit it directly; the
/// `set_name` field is renamed to `"set"` to match the documented contract.
#[derive(Debug, Clone, Serialize)]
pub struct SetInconsistency {
    /// In JSON output this appears as `"set"`.
    #[serde(rename = "set")]
    pub set_name: String,
    pub scope: InstallScope,
    pub kind: ArtifactKind,
    pub member: String,
    pub problem: SetProblem,
}

/// Whether some *other* active set in `sets` still claims `(kind, member)` —
/// the same reference-counting rule `sets::deactivate` uses, applied here
/// read-only against the loaded `SetsFile`.
fn held_by_active_set(kind: ArtifactKind, member: &str, this_set: &str, sets: &SetsFile) -> bool {
    sets.sets.iter().any(|(name, def)| {
        name != this_set
            && def.state == SetState::Active
            && def.members.iter().any(|m| m.kind == kind && m.name == member)
    })
}

/// Find every active-but-missing and inactive-but-lingering member across
/// `sets`, at `scope`. `is_installed` answers whether `(kind, name)` is
/// installed anywhere doctor's survey found it — the caller supplies this
/// from its own survey data so this function stays pure (no I/O).
pub fn set_inconsistencies(
    scope: InstallScope,
    sets: &SetsFile,
    is_installed: &dyn Fn(ArtifactKind, &str) -> bool,
) -> Vec<SetInconsistency> {
    let mut found = Vec::new();
    for (set_name, def) in &sets.sets {
        for m in &def.members {
            match def.state {
                SetState::Active if !is_installed(m.kind, &m.name) => {
                    found.push(SetInconsistency {
                        set_name: set_name.clone(),
                        scope,
                        kind: m.kind,
                        member: m.name.clone(),
                        problem: SetProblem::ActiveMissing,
                    });
                }
                SetState::Inactive
                    if is_installed(m.kind, &m.name)
                        && !held_by_active_set(m.kind, &m.name, set_name, sets) =>
                {
                    found.push(SetInconsistency {
                        set_name: set_name.clone(),
                        scope,
                        kind: m.kind,
                        member: m.name.clone(),
                        problem: SetProblem::InactiveLingering,
                    });
                }
                _ => {}
            }
        }
    }
    found
}

/// Load every scope's sets and cross-reference each member against what the
/// survey found installed, read-only (see `SETS.md`, "doctor integration").
/// `artifacts` already reflects every location/platform the survey walked, so
/// "installed" here means "present anywhere doctor's survey found it" —
/// consistent with `sets::show`'s own installed check.
pub(crate) fn collect_set_inconsistencies(
    scopes: &[InstallScope],
    artifacts: &[DoctorArtifact],
    ctx: &AppContext<'_>,
) -> Result<Vec<SetInconsistency>> {
    let installed: HashSet<(ArtifactKind, String)> =
        artifacts.iter().map(|a| (a.kind, a.name.clone())).collect();
    let is_installed =
        |kind: ArtifactKind, name: &str| installed.contains(&(kind, name.to_string()));

    let mut found = Vec::new();
    for &scope in scopes {
        let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
        found.extend(set_inconsistencies(scope, &sets, &is_installed));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SetDef;
    use crate::types::SetMember;
    use std::collections::BTreeMap;

    fn member(kind: ArtifactKind, name: &str) -> SetMember {
        SetMember {
            kind,
            name: name.to_string(),
            source: Some("guidelines".to_string()),
        }
    }

    fn sets_with(entries: Vec<(&str, SetState, Vec<SetMember>)>) -> SetsFile {
        let mut sets = BTreeMap::new();
        for (name, state, members) in entries {
            sets.insert(
                name.to_string(),
                SetDef {
                    description: None,
                    state,
                    members,
                },
            );
        }
        SetsFile { version: 1, sets }
    }

    #[test]
    fn active_set_missing_member_is_flagged() {
        let sets = sets_with(vec![(
            "rust-work",
            SetState::Active,
            vec![member(ArtifactKind::Agent, "rust-craftsperson")],
        )]);
        let found = set_inconsistencies(InstallScope::Global, &sets, &|_, _| false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].set_name, "rust-work");
        assert_eq!(found[0].problem, SetProblem::ActiveMissing);
    }

    #[test]
    fn active_set_with_installed_member_is_clean() {
        let sets = sets_with(vec![(
            "rust-work",
            SetState::Active,
            vec![member(ArtifactKind::Agent, "rust-craftsperson")],
        )]);
        let found = set_inconsistencies(InstallScope::Global, &sets, &|_, _| true);
        assert!(found.is_empty());
    }

    #[test]
    fn inactive_set_lingering_member_is_flagged() {
        let sets = sets_with(vec![(
            "client-ort",
            SetState::Inactive,
            vec![member(ArtifactKind::Skill, "ubiquity-router")],
        )]);
        let found = set_inconsistencies(InstallScope::Global, &sets, &|_, _| true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].problem, SetProblem::InactiveLingering);
    }

    #[test]
    fn inactive_set_member_not_installed_is_clean() {
        let sets = sets_with(vec![(
            "client-ort",
            SetState::Inactive,
            vec![member(ArtifactKind::Skill, "ubiquity-router")],
        )]);
        let found = set_inconsistencies(InstallScope::Global, &sets, &|_, _| false);
        assert!(found.is_empty());
    }

    #[test]
    fn inactive_member_held_by_active_set_is_not_flagged() {
        let shared = member(ArtifactKind::Skill, "foundry");
        let sets = sets_with(vec![
            ("blog", SetState::Inactive, vec![shared.clone()]),
            ("rust-work", SetState::Active, vec![shared]),
        ]);
        let found = set_inconsistencies(InstallScope::Global, &sets, &|_, _| true);
        assert!(
            found.is_empty(),
            "member held by another active set must not be flagged: {found:?}"
        );
    }
}
