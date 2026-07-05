//! `cmx doctor` — a read-only survey of the whole system installation.
//!
//! Doctor walks every platform's install directories (global, and project scope
//! when requested) and cross-references each per-platform lock file, then
//! classifies every artifact it finds. It is **read-only by contract**: it
//! mutates nothing and exists purely to make a disorganized installation
//! visible before any command changes a byte.
//!
//! ## Shared directories
//!
//! Several skills-only tools read the same physical `.agents/skills` directory.
//! Surveying naively per platform would report one on-disk skill many times.
//! Doctor instead keys the survey on the *resolved install directory*, scanning
//! each unique location once and attributing it to every platform that reads it.
//! An artifact is *tracked* if any attributed platform's lock file records it
//! with a matching checksum.

mod divergence;
mod set_consistency;
mod survey;
mod types;

pub use divergence::{DivergenceDetail, DivergenceMember, divergence_details, location_members};
pub use set_consistency::{SetInconsistency, SetProblem, set_inconsistencies};
pub use survey::survey;
pub use types::{ArtifactState, DoctorArtifact, DoctorReport, DoctorRow, MissingRow, StateCounts};

// Re-export private survey helpers for the test suite.
#[cfg(test)]
pub(crate) use survey::{
    LocationAgg, build_locations, group_rows, source_of, state_severity, survey_scopes,
};

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
