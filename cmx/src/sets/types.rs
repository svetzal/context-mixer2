//! Set data types.

use serde::Serialize;
use std::path::PathBuf;

use crate::platform::Platform;
use crate::types::{ArtifactKind, SetState};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Outcome of `cmx set create`.
#[derive(Debug)]
pub struct SetCreateResult {
    /// Name of the newly created set.
    pub name: String,
    /// Number of members the set was seeded with.
    pub member_count: usize,
    /// The `<source>:<plugin>` spec the set was seeded from, if `--from-plugin` was used.
    pub seeded_from: Option<String>,
}

/// One row of `cmx set list` — a set's identity, activation state, and cost.
#[derive(Debug, Serialize)]
pub struct SetListEntry {
    /// Name of the set.
    pub name: String,
    /// Whether the set is currently active, inactive, or partially active.
    pub state: SetState,
    /// Number of artifacts that belong to this set.
    pub member_count: usize,
    /// Total character count of the set's members' trigger descriptions — the
    /// context-footprint the set costs when active (see `SETS.md`,
    /// "Context-footprint reporting"). Members whose description could not be
    /// resolved contribute 0.
    pub footprint_chars: usize,
}

/// Result of `cmx set list` — every configured set's summary row.
#[derive(Debug, Serialize)]
pub struct SetListResult {
    /// One entry per configured set.
    pub entries: Vec<SetListEntry>,
}

/// A single member's identity and installation status within a set, as shown
/// by `cmx set show`.
#[derive(Debug, Serialize)]
pub struct SetMemberStatus {
    /// Whether this member is an agent or a skill.
    pub kind: ArtifactKind,
    /// Name of the member artifact.
    pub name: String,
    /// Name of the source the artifact resolves to, or `None` if unresolvable.
    pub source: Option<String>,
    /// Whether this member is currently installed anywhere in scope.
    pub installed: bool,
    /// This member's trigger-description character count, or `None` when it
    /// could not be resolved (source missing, artifact not found).
    pub footprint_chars: Option<usize>,
}

/// Result of `cmx set show` — a set's metadata plus per-member status.
#[derive(Debug, Serialize)]
pub struct SetShowResult {
    /// Name of the set.
    pub name: String,
    /// User-supplied description of the set, if any.
    pub description: Option<String>,
    /// Whether the set is currently active, inactive, or partially active.
    pub state: SetState,
    /// Per-member installation status.
    pub members: Vec<SetMemberStatus>,
    /// Sum of every resolvable member's `footprint_chars`.
    pub footprint_chars: usize,
}

/// Outcome of `cmx set add`.
#[derive(Debug)]
pub struct SetAddResult {
    /// Name of the set members were added to.
    pub set: String,
    /// Names newly added to the set.
    pub added: Vec<String>,
    /// Names that were already members and thus left untouched.
    pub already: Vec<String>,
}

/// Outcome of `cmx set remove`.
#[derive(Debug)]
pub struct SetRemoveResult {
    /// Name of the set members were removed from.
    pub set: String,
    /// Names removed from the set.
    pub removed: Vec<String>,
    /// Names that were not members and thus could not be removed.
    pub not_found: Vec<String>,
}

/// Outcome of `cmx set rename`.
#[derive(Debug)]
pub struct SetRenameResult {
    /// The set's previous name.
    pub old: String,
    /// The set's new name.
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

/// One platform-specific install target attempted (or planned) while
/// activating a set member.
#[derive(Debug)]
pub struct MemberActivateTarget {
    /// Platform this target installs to.
    pub platform: Platform,
    /// Path the artifact would be copied from.
    pub source_path: PathBuf,
    /// Path the artifact would be copied to.
    pub target_path: PathBuf,
    /// Version being installed, if the source declares one.
    pub version: Option<String>,
}

/// Per-member activation status, aggregating its outcome across all targeted
/// platforms.
#[derive(Debug)]
pub struct MemberActivateStatus {
    /// Whether this member is an agent or a skill.
    pub kind: ArtifactKind,
    /// Name of the member artifact.
    pub name: String,
    /// Overall install outcome for this member.
    pub outcome: MemberActivateOutcome,
    /// Per-platform targets considered for this member.
    pub targets: Vec<MemberActivateTarget>,
}

/// Result of a `cmx set activate` plan or apply run.
#[derive(Debug)]
pub struct SetActivateResult {
    /// Name of the set being activated.
    pub name: String,
    /// Per-member activation status.
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

/// One physical install location considered (or removed) while deactivating a
/// set member, shared by every platform whose install directory resolves to
/// the same path.
#[derive(Debug)]
pub struct MemberDeactivateTarget {
    /// All platforms whose install directory resolves to `artifact_path`.
    pub platforms: Vec<Platform>,
    /// Physical path the artifact is (or was) installed at.
    pub artifact_path: PathBuf,
    /// Concrete files whose local changes were discarded by `--force`.
    pub discarded_paths: Vec<PathBuf>,
}

/// Per-member deactivation status, aggregating its outcome across all
/// physical install locations.
#[derive(Debug)]
pub struct MemberDeactivateStatus {
    /// Whether this member is an agent or a skill.
    pub kind: ArtifactKind,
    /// Name of the member artifact.
    pub name: String,
    /// Overall uninstall outcome for this member.
    pub outcome: MemberDeactivateOutcome,
    /// Per-location targets considered for this member.
    pub targets: Vec<MemberDeactivateTarget>,
}

/// Result of a `cmx set deactivate` plan or apply run.
#[derive(Debug)]
pub struct SetDeactivateResult {
    /// Name of the set being deactivated.
    pub name: String,
    /// Per-member deactivation status.
    pub members: Vec<MemberDeactivateStatus>,
    /// True when a drift-blocked member (no `--force`) prevented a full deactivation.
    pub any_blocked: bool,
    /// True when `--apply` was passed and the plan was executed.
    pub apply: bool,
}

/// Result of a `cmx set delete` plan or apply run.
#[derive(Debug)]
pub struct SetDeleteResult {
    /// Name of the set being deleted.
    pub name: String,
    /// Whether `--purge` (uninstall members not retained by another set) was
    /// requested.
    pub purge: bool,
    /// True when `--apply` was passed and the plan was executed.
    pub apply: bool,
    /// True when the set definition was actually removed.
    pub deleted: bool,
    /// The purge's deactivation result, present only when `purge` was set.
    pub deactivate: Option<SetDeactivateResult>,
}
