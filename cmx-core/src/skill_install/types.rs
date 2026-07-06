use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::platform::Platform;
use crate::skill_fs::SkillFile;
use crate::types::InstallScope;

// ---------------------------------------------------------------------------
// Public API types
// ---------------------------------------------------------------------------

/// Identity of the embedding tool — name and semver version string.
#[derive(Debug, Clone)]
pub struct ToolIdentity {
    pub name: String,
    pub version: String,
}

impl ToolIdentity {
    /// Construct a tool identity from a name and version string.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// A skill bundled inside a tool binary (via `include_str!` or similar).
pub struct BundledSkill {
    pub files: Vec<SkillFile>,
}

impl BundledSkill {
    /// Construct from a list of files (e.g. assembled by the embedding tool from
    /// `include_str!` or `include_bytes!` calls).
    pub fn from_files(files: Vec<SkillFile>) -> Self {
        Self { files }
    }

    /// Convenience constructor for the common single-`SKILL.md` case.
    ///
    /// Builds a bundle containing exactly one file at path `SKILL.md` with the
    /// given content. Use `from_files` when the skill includes additional files.
    pub fn single_md(content: &str) -> Self {
        Self {
            files: vec![SkillFile {
                rel_path: PathBuf::from("SKILL.md"),
                bytes: content.as_bytes().to_vec(),
            }],
        }
    }

    /// Returns `true` when the bundle contains a `SKILL.md` at the root level
    /// (i.e. `rel_path == "SKILL.md"`).
    pub fn has_skill_md(&self) -> bool {
        self.files.iter().any(|f| f.rel_path.as_os_str() == "SKILL.md")
    }
}

/// Installation scope — global (user-wide) or local (project-scoped).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Global,
    Local,
}

impl Scope {
    pub fn to_install_scope(self) -> InstallScope {
        match self {
            Scope::Global => InstallScope::Global,
            Scope::Local => InstallScope::Local,
        }
    }
}

// ---------------------------------------------------------------------------
// Plan types
// ---------------------------------------------------------------------------

/// The action to take for a single target platform during an install.
///
/// # Non-exhaustive
///
/// This enum is `#[non_exhaustive]`: new action variants may be added in
/// future minor releases. Embedders should render actions via the `Display`
/// impl on `Report`/`InstallPlan` or match on specific variants they care
/// about with a catch-all `_` arm. The `will_write()` and `is_blocked()`
/// helpers cover the two common branching points without requiring exhaustive
/// matching.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum TargetAction {
    /// First-time install (no existing copy).
    Install,
    /// Overwrite an older installed version.
    Update {
        /// The previously installed version string (if known).
        from: Option<String>,
    },
    /// Already installed at the same version and checksum — nothing to do.
    Skip,
    /// Same version but the on-disk content differs from the bundled content,
    /// and `force` was not requested.
    DriftedSkip { installed: String },
    /// The installed version is newer than the bundled version, and `force` was
    /// not requested.
    RefuseNewer { installed: String },
    /// The installed version is newer, but `force` was requested — downgrade.
    Downgrade { from: String },
}

impl TargetAction {
    /// Whether this action will write files to disk.
    pub fn will_write(&self) -> bool {
        matches!(self, Self::Install | Self::Update { .. } | Self::Downgrade { .. })
    }

    /// Whether this action blocks the install from proceeding.
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::RefuseNewer { .. })
    }
}

/// A single file to be written, with its relative and absolute destination paths.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    /// Relative path within the skill directory (e.g. `SKILL.md`).
    pub rel_path: PathBuf,
    /// Absolute (or scope-relative) destination path on disk.
    pub dest_path: PathBuf,
}

/// The plan for a single target platform.
#[derive(Debug)]
pub struct TargetPlan {
    pub platform: Platform,
    pub scope: InstallScope,
    pub dest_dir: PathBuf,
    pub files: Vec<PlannedFile>,
    pub action: TargetAction,
    /// Whether this platform is in the cmx-managed set.
    pub cmx_managed: bool,
}

/// The full installation plan — computed from source metadata, with no writes.
#[derive(Debug)]
pub struct InstallPlan {
    pub tool: ToolIdentity,
    pub scope: InstallScope,
    pub source_checksum: String,
    /// Whether cmx is managing this machine (config or non-empty lock exists).
    pub cmx_present: bool,
    pub force: bool,
    pub targets: Vec<TargetPlan>,
}

impl InstallPlan {
    /// Returns `true` if any target action is blocked (e.g. `RefuseNewer`).
    pub fn is_blocked(&self) -> bool {
        self.targets.iter().any(|t| t.action.is_blocked())
    }

    /// The number of targets that will write files to disk.
    pub fn write_count(&self) -> usize {
        self.targets.iter().filter(|t| t.action.will_write()).count()
    }
}

// ---------------------------------------------------------------------------
// Apply result types
// ---------------------------------------------------------------------------

/// The outcome for a single target platform after `apply`.
///
/// Both written and skipped targets appear in `Report::targets`; the `action`
/// field distinguishes them. Use `Report::applied()` / `Report::skipped()` for
/// filtered views.
#[derive(Debug)]
pub struct TargetOutcome {
    pub platform: Platform,
    pub dest_dir: PathBuf,
    pub action: TargetAction,
    /// Number of files written to disk (0 for skipped targets).
    pub files_written: usize,
    /// Checksum recorded in the lock file. `Some` for written targets, `None`
    /// for skipped targets (no lock entry was touched).
    pub installed_checksum: Option<String>,
    /// Concrete target files whose local changes were discarded by `--force`.
    pub discarded_paths: Vec<PathBuf>,
}

/// The full report returned by [`SkillInstaller::apply`].
///
/// All targets — both written and skipped — are captured in `targets` so that
/// skipped targets retain their `dest_dir` and the `action` discriminant
/// distinguishes an ordinary up-to-date skip from a `DriftedSkip` (local edits
/// preserved). Use the `applied()` and `skipped()` iterators for filtered views.
#[derive(Debug)]
pub struct Report {
    pub tool: ToolIdentity,
    pub scope: InstallScope,
    /// Every platform that was considered, written or not.
    pub targets: Vec<TargetOutcome>,
    /// Whether a source entry was registered in sources.json.
    pub source_registered: bool,
}

impl Report {
    /// Targets where files were written to disk (Install / Update / Downgrade).
    pub fn applied(&self) -> impl Iterator<Item = &TargetOutcome> {
        self.targets.iter().filter(|o| o.action.will_write())
    }

    /// Targets where no files were written (`Skip` / `DriftedSkip` / `RefuseNewer`).
    pub fn skipped(&self) -> impl Iterator<Item = &TargetOutcome> {
        self.targets.iter().filter(|o| !o.action.will_write())
    }
}

/// Internal helper: tracks which dirs need writing and which have been cleared.
pub(super) struct PreparedWrites {
    pub(super) dirs_to_write: BTreeSet<PathBuf>,
    pub(super) discarded_paths_by_dir: BTreeMap<PathBuf, Vec<PathBuf>>,
}

// ---------------------------------------------------------------------------
// Status types
// ---------------------------------------------------------------------------

/// Status of a skill on a single platform.
#[derive(Debug)]
pub struct TargetStatus {
    pub platform: Platform,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub drifted: bool,
    pub tracked: bool,
}

/// Status of a skill across all relevant platforms.
#[derive(Debug)]
pub struct Status {
    pub tool_name: String,
    pub scope: InstallScope,
    pub targets: Vec<TargetStatus>,
}

// ---------------------------------------------------------------------------
// Remove result types
// ---------------------------------------------------------------------------

/// The result of removing a skill.
#[derive(Debug)]
pub struct RemoveReport {
    pub tool_name: String,
    pub scope: InstallScope,
    pub removed_dirs: Vec<PathBuf>,
    pub platforms_cleared: Vec<Platform>,
    pub source_unregistered: bool,
    pub was_on_disk: bool,
    pub was_tracked: bool,
}
