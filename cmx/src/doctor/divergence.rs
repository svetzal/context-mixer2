use std::path::PathBuf;

use crate::platform::Platform;

use super::types::{DoctorArtifact, DoctorRow};

/// Per-location data for one copy of a diverged artifact.
#[derive(Debug, Clone)]
pub struct DivergenceMember {
    pub location: PathBuf,
    /// A deterministic representative for the location, used by
    /// `doctor --json`'s singular `platform` field.
    pub platform: Option<Platform>,
    /// Every surveyed platform that reads this install location. The human
    /// doctor table renders one `platform@version` pair per entry here.
    pub platforms: Vec<Platform>,
    pub version: Option<String>,
    pub state_label: &'static str,
}

/// Per-artifact divergence breakdown — the data needed to render the detail
/// lines under the summary. Pure: no I/O, computed from borrowed report data.
#[derive(Debug, Clone)]
pub struct DivergenceDetail {
    pub name: String,
    /// True when the copies also differ in *state* (not just version).
    pub states_differ: bool,
    pub members: Vec<DivergenceMember>,
}

/// Build the per-location breakdown for one logical artifact — every row
/// matching its `(kind, name, scope)`, sorted by location. Unfiltered by
/// `diverged`: callers that only care about diverged artifacts filter before
/// calling this; `doctor_json` wants the same shape for every artifact.
pub fn location_members(artifact: &DoctorArtifact, rows: &[DoctorRow]) -> Vec<DivergenceMember> {
    let mut members: Vec<&DoctorRow> = rows
        .iter()
        .filter(|r| r.kind == artifact.kind && r.name == artifact.name && r.scope == artifact.scope)
        .collect();
    members.sort_by(|x, y| x.location.cmp(&y.location));
    members
        .into_iter()
        .map(|r| DivergenceMember {
            location: r.location.clone(),
            platform: r.platforms.first().copied(),
            platforms: r.platforms.clone(),
            version: r.version.clone(),
            state_label: r.state.label(),
        })
        .collect()
}

/// Build divergence details for every diverged artifact in `shown`.
///
/// Pure function — no I/O. The display layer calls this to obtain the
/// structured data it needs to render the per-location breakdown.
pub fn divergence_details(shown: &[&DoctorArtifact], rows: &[DoctorRow]) -> Vec<DivergenceDetail> {
    shown
        .iter()
        .filter(|a| a.diverged)
        .map(|a| {
            let members = location_members(a, rows);
            let states_differ = members
                .iter()
                .map(|m| m.state_label)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1;
            DivergenceDetail {
                name: a.name.clone(),
                states_differ,
                members,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::platform::Platform;
    use crate::types::{ArtifactKind, InstallScope};

    use super::{divergence_details, location_members};
    use crate::doctor::tests::make_row;
    use crate::doctor::types::{ArtifactState, DoctorArtifact};

    fn make_artifact(
        kind: ArtifactKind,
        name: &str,
        diverged: bool,
        state: ArtifactState,
    ) -> DoctorArtifact {
        DoctorArtifact {
            kind,
            name: name.to_string(),
            scope: InstallScope::Global,
            state,
            version: None,
            versions: vec![],
            tools: vec![],
            source: None,
            locations: vec![],
            diverged,
        }
    }

    // -----------------------------------------------------------------------
    // location_members
    // -----------------------------------------------------------------------

    #[test]
    fn location_members_filters_by_kind_name_scope() {
        let skill_row = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        let agent_row = make_row(ArtifactKind::Agent, "alpha", ArtifactState::Tracked, "sha256:y");
        let other_skill = make_row(ArtifactKind::Skill, "beta", ArtifactState::Tracked, "sha256:z");

        let artifact = make_artifact(ArtifactKind::Skill, "alpha", false, ArtifactState::Tracked);
        let rows = vec![skill_row, agent_row, other_skill];
        let members = location_members(&artifact, &rows);

        assert_eq!(members.len(), 1, "only the matching (kind=Skill, name=alpha) row must appear");
    }

    #[test]
    fn location_members_sorted_by_location() {
        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row_b.location = PathBuf::from("/z/path");

        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row_a.location = PathBuf::from("/a/path");

        let artifact = make_artifact(ArtifactKind::Skill, "alpha", false, ArtifactState::Tracked);
        let members = location_members(&artifact, &[row_b, row_a]);

        assert_eq!(members[0].location, PathBuf::from("/a/path"));
        assert_eq!(members[1].location, PathBuf::from("/z/path"));
    }

    #[test]
    fn location_members_platform_is_first_of_platforms() {
        let mut row = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row.platforms = vec![Platform::Claude, Platform::Codex];

        let artifact = make_artifact(ArtifactKind::Skill, "alpha", false, ArtifactState::Tracked);
        let members = location_members(&artifact, &[row]);

        assert_eq!(members[0].platform, Some(Platform::Claude));
        assert_eq!(members[0].platforms, vec![Platform::Claude, Platform::Codex]);
    }

    // -----------------------------------------------------------------------
    // divergence_details
    // -----------------------------------------------------------------------

    #[test]
    fn divergence_details_skips_non_diverged_artifacts() {
        let artifact = make_artifact(
            ArtifactKind::Skill,
            "alpha",
            false, /* diverged=false */
            ArtifactState::Tracked,
        );
        let rows = vec![make_row(
            ArtifactKind::Skill,
            "alpha",
            ArtifactState::Tracked,
            "sha256:x",
        )];
        let details = divergence_details(&[&artifact], &rows);
        assert!(details.is_empty(), "non-diverged artifact must be filtered out");
    }

    #[test]
    fn divergence_details_includes_diverged_artifacts() {
        let artifact = make_artifact(
            ArtifactKind::Skill,
            "alpha",
            true, /* diverged */
            ArtifactState::Tracked,
        );
        let rows = vec![make_row(
            ArtifactKind::Skill,
            "alpha",
            ArtifactState::Tracked,
            "sha256:x",
        )];
        let details = divergence_details(&[&artifact], &rows);
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].name, "alpha");
    }

    #[test]
    fn divergence_details_states_differ_false_when_all_same_state() {
        let artifact = make_artifact(ArtifactKind::Skill, "alpha", true, ArtifactState::Tracked);
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v1");
        row_a.location = PathBuf::from("/path/a");
        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v2");
        row_b.location = PathBuf::from("/path/b");
        let details = divergence_details(&[&artifact], &[row_a, row_b]);
        assert!(!details[0].states_differ, "copies with same state must not set states_differ");
    }

    #[test]
    fn divergence_details_states_differ_true_when_states_differ() {
        let artifact = make_artifact(ArtifactKind::Skill, "alpha", true, ArtifactState::Drifted);
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v1");
        row_a.location = PathBuf::from("/path/a");
        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Drifted, "sha256:v2");
        row_b.location = PathBuf::from("/path/b");
        let details = divergence_details(&[&artifact], &[row_a, row_b]);
        assert!(details[0].states_differ, "copies with different states must set states_differ");
    }
}
