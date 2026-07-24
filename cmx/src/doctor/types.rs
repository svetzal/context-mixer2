//! Doctor result/report types.

use std::path::PathBuf;

use serde::Serialize;

use super::set_consistency::SetInconsistency;
use crate::platform::Platform;
use crate::types::{ArtifactKind, InstallScope};

/// Classification of an installed artifact relative to the lock files that
/// should track it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactState {
    /// Present on disk and recorded in a lock file with a matching checksum.
    Tracked,
    /// Present on disk and in a lock file, but the on-disk copy was edited after
    /// install (checksum mismatch).
    Drifted,
    /// Present on disk with no lock entry, but a registered source provides an
    /// artifact of the same kind and name. Installed out-of-band — the fix is to
    /// track it via `install`, *not* adopt it as private.
    Untracked,
    /// Present on disk with no lock entry and **no** registered source provides
    /// it — a genuinely hand-authored artifact. The adopt candidate.
    Orphaned,
    /// Present on disk but declared external in config — managed by another tool,
    /// not cmx. Reported for visibility but never an issue.
    External,
}

impl ArtifactState {
    /// The lowercase label used in doctor's human-readable and JSON output.
    pub fn label(self) -> &'static str {
        match self {
            ArtifactState::Tracked => "tracked",
            ArtifactState::Drifted => "drifted",
            ArtifactState::Untracked => "untracked",
            ArtifactState::Orphaned => "orphaned",
            ArtifactState::External => "external",
        }
    }
}

/// One installed artifact discovered on disk during the survey, at a single
/// install location. This is the raw per-location unit; for the user-facing view
/// these are grouped into [`DoctorArtifact`] (one logical artifact across all the
/// tools it's installed for).
#[derive(Debug, Clone)]
pub struct DoctorRow {
    /// Whether this is an agent or a skill.
    pub kind: ArtifactKind,
    /// The artifact's name.
    pub name: String,
    /// Whether this location is global (user-wide) or local (project-scoped).
    pub scope: InstallScope,
    /// The resolved install directory the artifact was found in.
    pub location: PathBuf,
    /// Every platform that reads this location (more than one for the shared
    /// `.agents/skills` cohort). Used by adopt to record provenance.
    pub platforms: Vec<Platform>,
    /// The platforms whose lock file actually records this artifact — i.e. the
    /// tools cmx *manages* it for, a subset of `platforms`. Empty for artifacts
    /// with no lock entry (orphaned/untracked/external).
    pub tracked_for: Vec<Platform>,
    /// This location's classification relative to the lock files that should track it.
    pub state: ArtifactState,
    /// The version recorded for this location, if any.
    pub version: Option<String>,
    /// The source this came from: the lock entry's repo when tracked/drifted, or
    /// the providing source when untracked. `None` for orphaned/external.
    pub source: Option<String>,
    /// The artifact's current on-disk content checksum (SHA-256). Drives
    /// content-based divergence: copies whose bytes differ are flagged diverged,
    /// independent of their version or tracking state — so a genuine content
    /// difference between two unversioned copies is caught, while byte-identical
    /// copies that merely differ in tracking state are not.
    pub content_checksum: String,
}

/// One *logical* artifact — a `(kind, name, scope)` grouped across every install
/// location cmx found it in. A skill projected to several tools is **one**
/// `DoctorArtifact` listing all those tools, not N "duplicates".
#[derive(Debug, Clone)]
pub struct DoctorArtifact {
    /// Whether this is an agent or a skill.
    pub kind: ArtifactKind,
    /// The artifact's name.
    pub name: String,
    /// Whether this artifact is global (user-wide) or local (project-scoped).
    pub scope: InstallScope,
    /// Consolidated state. When the copies disagree this is the most actionable
    /// one (see [`diverged`](Self::diverged)).
    pub state: ArtifactState,
    /// The version, when all copies agree; `None` if they differ or carry none.
    pub version: Option<String>,
    /// The distinct versions present across copies, sorted. One entry (or none)
    /// when copies agree; several when they diverge — lets the display name the
    /// skew (e.g. `3.2.0 / 3.3.0`) instead of an opaque `-`.
    pub versions: Vec<String>,
    /// The platforms cmx *manages* this artifact for (has a lock entry), unioned
    /// across its locations. Not every tool that merely reads a shared directory
    /// — only those cmx tracks it for. Empty when nothing tracks it.
    pub tools: Vec<Platform>,
    /// The source it came from (lock provenance), when all copies agree.
    pub source: Option<String>,
    /// The distinct install locations it occupies.
    pub locations: Vec<PathBuf>,
    /// True when the copies' **content differs** across locations (distinct
    /// checksums). This is the multi-location situation worth flagging — the
    /// copies have genuinely drifted apart and need reconciling. Byte-identical
    /// copies are just one skill installed to many tools, even when their
    /// tracking state differs (e.g. tracked for one tool, untracked for
    /// another) — that asymmetry surfaces through the per-copy state, not here.
    pub diverged: bool,
}

/// A lock entry whose artifact is no longer present on disk.
#[derive(Debug, Clone)]
pub struct MissingRow {
    /// Whether this is an agent or a skill.
    pub kind: ArtifactKind,
    /// The artifact's name.
    pub name: String,
    /// Whether the missing lock entry is global (user-wide) or local (project-scoped).
    pub scope: InstallScope,
    /// The platform whose lock file records this artifact as installed.
    pub platform: Platform,
}

/// The full read-only survey result.
///
/// `rows` is the raw per-location view (used by adopt and for detail);
/// `artifacts` is the grouped logical view (one entry per skill, listing the
/// tools it's installed for) used for display and counts.
#[derive(Debug, Default)]
pub struct DoctorReport {
    /// The raw per-location view: one entry per install location found on disk.
    pub rows: Vec<DoctorRow>,
    /// The grouped logical view: one entry per `(kind, name, scope)`, listing
    /// every tool it's installed for.
    pub artifacts: Vec<DoctorArtifact>,
    /// Lock entries whose artifact is no longer present on disk.
    pub missing: Vec<MissingRow>,
    /// Whether project (local) scope was included in the survey.
    pub included_local: bool,
    /// How many platforms the survey actually inspected — every supported
    /// platform, or just the managed set when one is configured.
    pub surveyed_platforms: usize,
    /// `true` when the survey was narrowed to an explicit managed set (so the
    /// header can say so rather than implying the whole field was checked).
    pub scoped_to_managed: bool,
    /// Display hint: when `true`, the full inventory is shown; otherwise only
    /// artifacts that need attention (the default — `doctor` is for problems).
    pub show_all: bool,
    /// Set/installed-state mismatches found by the Phase 3 set-consistency
    /// check (see `SETS.md`, "doctor integration") — active sets with a
    /// missing member, or inactive sets with a member still lingering
    /// installed on their behalf.
    pub set_inconsistencies: Vec<SetInconsistency>,
}

impl DoctorReport {
    /// Whether a logical artifact needs attention — drifted/untracked/orphaned,
    /// or *any* artifact whose copies diverge across locations.
    ///
    /// A clean external or tracked artifact is fine: another tool managing it, or
    /// cmx managing it consistently, is the steady state. But a **divergence** —
    /// two copies at different versions or states — is a real anomaly worth
    /// surfacing whoever owns it; cmx just can't be the one to re-sync an external
    /// one (its owning tool must). So divergence is always a problem; only a
    /// *consistent* external/tracked artifact is healthy.
    pub fn is_problem(a: &DoctorArtifact) -> bool {
        match a.state {
            ArtifactState::External | ArtifactState::Tracked => a.diverged,
            _ => true,
        }
    }
}

/// Per-state tallies for the summary line. Counts are over *logical* artifacts.
/// Derives `Serialize` so `doctor --json` can emit it directly as the
/// `"summary"` object without a parallel hand-built mapping.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct StateCounts {
    /// Count of artifacts tracked and matching their lock checksum.
    pub tracked: usize,
    /// Count of artifacts tracked but edited after install.
    pub drifted: usize,
    /// Count of artifacts on disk with no lock entry but a matching source.
    pub untracked: usize,
    /// Count of hand-authored artifacts with no lock entry and no matching source.
    pub orphaned: usize,
    /// Count of artifacts declared external (managed by another tool).
    pub external: usize,
    /// Count of lock entries whose artifact is no longer present on disk.
    pub missing: usize,
    /// Logical artifacts whose copies disagree across locations.
    pub diverged: usize,
    /// Set/installed-state mismatches (see [`DoctorReport::set_inconsistencies`]).
    pub set_inconsistent: usize,
}

impl DoctorReport {
    /// Tally logical artifacts by state for the summary line.
    pub fn counts(&self) -> StateCounts {
        let mut c = StateCounts {
            missing: self.missing.len(),
            set_inconsistent: self.set_inconsistencies.len(),
            ..StateCounts::default()
        };
        for a in &self.artifacts {
            match a.state {
                ArtifactState::Tracked => c.tracked += 1,
                ArtifactState::Drifted => c.drifted += 1,
                ArtifactState::Untracked => c.untracked += 1,
                ArtifactState::Orphaned => c.orphaned += 1,
                ArtifactState::External => c.external += 1,
            }
            // Every divergence counts — including external ones, which are a real
            // anomaly even if their owning tool (not cmx) must re-sync them.
            if a.diverged {
                c.diverged += 1;
            }
        }
        c
    }

    /// Whether the survey found anything that needs attention.
    ///
    /// Drift, untracked, orphaned, missing, and *diverged* (copies that
    /// disagree across locations) are issues. `tracked` and `external` (managed
    /// by another tool) are not — and a skill consistently installed to many
    /// tools is just that, not a problem.
    pub fn has_issues(&self) -> bool {
        !self.missing.is_empty()
            || !self.set_inconsistencies.is_empty()
            || self.artifacts.iter().any(Self::is_problem)
    }
}
