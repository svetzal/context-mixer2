use serde::Serialize;
use std::path::PathBuf;

use crate::platform::Platform;
use crate::types::{ArtifactKind, SetState};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SetCreateResult {
    pub name: String,
    pub member_count: usize,
    /// The `<source>:<plugin>` spec the set was seeded from, if `--from-plugin` was used.
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
pub struct SetRenameResult {
    pub old: String,
    pub new: String,
}

/// Per-member outcome of an `activate` plan or apply run.
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
pub struct MemberActivateTarget {
    pub platform: Platform,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub version: Option<String>,
}

#[derive(Debug)]
pub struct MemberActivateStatus {
    pub kind: ArtifactKind,
    pub name: String,
    pub outcome: MemberActivateOutcome,
    pub targets: Vec<MemberActivateTarget>,
}

#[derive(Debug)]
pub struct SetActivateResult {
    pub name: String,
    pub members: Vec<MemberActivateStatus>,
    /// True when any member was unresolvable or failed to install everywhere.
    pub any_failed: bool,
    /// True when `--apply` was passed and the plan was executed.
    pub apply: bool,
}

/// Per-member outcome of a `deactivate` plan or apply run.
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
pub struct MemberDeactivateTarget {
    pub platform: Platform,
    pub artifact_path: PathBuf,
    pub discarded_paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct MemberDeactivateStatus {
    pub kind: ArtifactKind,
    pub name: String,
    pub outcome: MemberDeactivateOutcome,
    pub targets: Vec<MemberDeactivateTarget>,
}

#[derive(Debug)]
pub struct SetDeactivateResult {
    pub name: String,
    pub members: Vec<MemberDeactivateStatus>,
    /// True when a drift-blocked member (no `--force`) prevented a full deactivation.
    pub any_blocked: bool,
    /// True when `--apply` was passed and the plan was executed.
    pub apply: bool,
}

#[derive(Debug)]
pub struct SetDeleteResult {
    pub name: String,
    pub purge: bool,
    pub apply: bool,
    pub deleted: bool,
    pub deactivate: Option<SetDeactivateResult>,
}
