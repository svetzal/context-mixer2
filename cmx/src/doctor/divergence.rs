use std::path::PathBuf;

use super::types::{DoctorArtifact, DoctorRow};

/// Per-location data for one copy of a diverged artifact.
#[derive(Debug, Clone)]
pub struct DivergenceMember {
    pub location: PathBuf,
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

/// Build divergence details for every diverged artifact in `shown`.
///
/// Pure function — no I/O. The display layer calls this to obtain the
/// structured data it needs to render the per-location breakdown.
pub fn divergence_details(shown: &[&DoctorArtifact], rows: &[DoctorRow]) -> Vec<DivergenceDetail> {
    shown
        .iter()
        .filter(|a| a.diverged)
        .map(|a| {
            let mut members: Vec<&DoctorRow> = rows
                .iter()
                .filter(|r| r.kind == a.kind && r.name == a.name && r.scope == a.scope)
                .collect();
            members.sort_by(|x, y| x.location.cmp(&y.location));
            let states_differ = members
                .iter()
                .map(|r| r.state.label())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 1;
            DivergenceDetail {
                name: a.name.clone(),
                states_differ,
                members: members
                    .into_iter()
                    .map(|r| DivergenceMember {
                        location: r.location.clone(),
                        version: r.version.clone(),
                        state_label: r.state.label(),
                    })
                    .collect(),
            }
        })
        .collect()
}
