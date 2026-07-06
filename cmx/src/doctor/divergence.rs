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
