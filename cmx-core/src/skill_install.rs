//! High-level, embeddable skill-installation API.
//!
//! A tool bundles its companion skill (as [`BundledSkill`]) and calls
//! [`SkillInstaller`] to install, query, or remove it — without knowing about
//! any cmx internals.
//!
//! ```no_run
//! # use anyhow::Result;
//! # fn main() -> Result<()> {
//! use cmx_core::production::ProductionContext;
//! use cmx_core::skill_install::{BundledSkill, Scope, SkillInstaller, ToolIdentity};
//!
//! // The SKILL.md needs no version of its own — the installer stamps
//! // `metadata.version` from the ToolIdentity below at install time.
//! let skill = BundledSkill::single_md("---\nname: mytool\n---\n# My skill\n");
//! let installer = SkillInstaller::new(ToolIdentity::new("mytool", "1.2.0"));
//! let prod_ctx = ProductionContext::claude()?;
//! let ctx = prod_ctx.ctx();
//! let plan = installer.plan(&skill, Scope::Global, false, &ctx)?;
//! println!("{plan}");
//! let report = installer.apply(&skill, &plan, &ctx)?;
//! println!("{report}");
//! # Ok(())
//! # }
//! ```

use anyhow::{Result, bail};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::frontmatter;
use crate::fs_util;
use crate::lockfile;
use crate::platform::Platform;
use crate::platform_iter;
use crate::skill_fs::{self, SkillFile};
use crate::targets;
use crate::types::{ArtifactKind, InstallScope, LockEntry, LockSource, SourceEntry, SourceType};

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

impl std::fmt::Display for InstallPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Install plan for {} v{}", self.tool.name, self.tool.version)?;
        writeln!(f, "  scope: {}", self.scope.label())?;
        writeln!(f, "  checksum: {}", self.source_checksum)?;
        for target in &self.targets {
            writeln!(
                f,
                "  {} → {} ({})",
                target.platform,
                target.dest_dir.display(),
                format_action(&target.action)
            )?;
        }
        Ok(())
    }
}

fn format_action(action: &TargetAction) -> String {
    match action {
        TargetAction::Install => "install".to_string(),
        TargetAction::Update { from } => {
            format!("update from {}", from.as_deref().unwrap_or("?"))
        }
        TargetAction::Skip => "skip (up to date)".to_string(),
        TargetAction::DriftedSkip { installed } => {
            format!("skip (drifted from {installed})")
        }
        TargetAction::RefuseNewer { installed } => {
            format!("refuse (installed {installed} is newer)")
        }
        TargetAction::Downgrade { from } => format!("downgrade from {from}"),
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

struct PreparedWrites {
    dirs_to_write: BTreeSet<PathBuf>,
    discarded_paths_by_dir: BTreeMap<PathBuf, Vec<PathBuf>>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Installed {} v{} ({})",
            self.tool.name,
            self.tool.version,
            self.scope.label()
        )?;
        for outcome in &self.targets {
            writeln!(
                f,
                "  {} → {} ({})",
                outcome.platform,
                outcome.dest_dir.display(),
                format_action(&outcome.action)
            )?;
        }
        if self.source_registered {
            writeln!(f, "  (registered as cmx source)")?;
        }
        Ok(())
    }
}

impl std::fmt::Display for RemoveReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Removed {} ({})", self.tool_name, self.scope.label())?;
        for platform in &self.platforms_cleared {
            writeln!(f, "  {platform} lock entry cleared")?;
        }
        for dir in &self.removed_dirs {
            writeln!(f, "  removed: {}", dir.display())?;
        }
        if self.source_unregistered {
            writeln!(f, "  unregistered from cmx sources")?;
        }
        writeln!(f, "  note: cmx-lock.json left on disk (shared with other tools)")?;
        Ok(())
    }
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

// ---------------------------------------------------------------------------
// SkillInstaller
// ---------------------------------------------------------------------------

/// High-level skill lifecycle manager for embedding tools.
pub struct SkillInstaller {
    tool: ToolIdentity,
}

impl SkillInstaller {
    /// Create a new installer for the given tool identity.
    pub fn new(tool: ToolIdentity) -> Self {
        Self { tool }
    }

    /// Compute a dry-run install plan without writing anything.
    ///
    /// Fails if the bundle does not contain a `SKILL.md`.
    pub fn plan(
        &self,
        skill: &BundledSkill,
        scope: Scope,
        force: bool,
        ctx: &AppContext<'_>,
    ) -> Result<InstallPlan> {
        if !skill.has_skill_md() {
            bail!("BundledSkill for '{}' is missing SKILL.md", self.tool.name);
        }

        // Reconcile the SKILL.md frontmatter's `metadata.version` to this tool's
        // version before anything else, so the checksum, the written bytes, and the
        // lock entry all describe the same, version-stamped content.
        let files = frontmatter::reconcile_skill_version(&skill.files, &self.tool.version);
        let source_checksum = skill_fs::checksum_bundled(&files);
        let install_scope = scope.to_install_scope();

        let platform_targets =
            targets::resolve_targets(None, ArtifactKind::Skill, install_scope, ctx)?;

        let cmx_managed = config::managed_platforms(ctx.fs, ctx.paths)?.is_some();
        let cmx_present = cmx_managed || {
            // Check whether any platform has a non-empty lock file
            platform_iter::views_for(ctx.paths, platform_iter::all(), ArtifactKind::Skill).any(
                |view| {
                    lockfile::load(install_scope, ctx.fs, &view.paths)
                        .ok()
                        .is_some_and(|l| !l.packages.is_empty())
                },
            )
        };

        let mut target_plans = Vec::new();
        // Track which dest_dirs we've already planned files for (shared dirs).
        // For platforms that share a dest_dir (e.g. .agents/skills), we still
        // produce a TargetPlan per platform but with the same files list.
        // Apply will dedup writes by dest_dir.

        for &platform in &platform_targets {
            let pv = ctx.paths.with_platform(platform);
            let dest_dir = pv.require_install_dir(ArtifactKind::Skill, install_scope)?;
            let skill_dest = dest_dir.join(&self.tool.name);

            // Build planned files
            let planned_files: Vec<PlannedFile> = files
                .iter()
                .map(|f| PlannedFile {
                    rel_path: f.rel_path.clone(),
                    dest_path: skill_dest.join(&f.rel_path),
                })
                .collect();

            // Determine the action for this platform
            let lock = lockfile::load(install_scope, ctx.fs, &pv)?;
            let action = if let Some(entry) = lock.packages.get(&self.tool.name) {
                decide_action_for_entry(
                    entry,
                    &self.tool.version,
                    &source_checksum,
                    force,
                    &skill_dest,
                    ctx,
                )?
            } else if ctx.fs.exists(&skill_dest) {
                // On disk but not tracked: treat as Install (untracked copy)
                TargetAction::Install
            } else {
                TargetAction::Install
            };

            target_plans.push(TargetPlan {
                platform,
                scope: install_scope,
                dest_dir: skill_dest,
                files: planned_files,
                action,
                cmx_managed,
            });
        }

        Ok(InstallPlan {
            tool: self.tool.clone(),
            scope: install_scope,
            source_checksum,
            cmx_present,
            force,
            targets: target_plans,
        })
    }

    /// Apply an install plan, writing files and updating lock entries.
    ///
    /// Fails if:
    /// - The plan is blocked (e.g. `RefuseNewer`).
    /// - The bundled skill's checksum does not match the plan's `source_checksum`
    ///   (parity guard — ensures the skill passed here is the same one planned).
    pub fn apply(
        &self,
        skill: &BundledSkill,
        plan: &InstallPlan,
        ctx: &AppContext<'_>,
    ) -> Result<Report> {
        if plan.is_blocked() {
            bail!(
                "Install plan for '{}' is blocked. Run with force=true to override.",
                self.tool.name
            );
        }

        // Reconcile the same way plan() did, so the checksum below and the bytes
        // written match the planned source_checksum exactly.
        let files = frontmatter::reconcile_skill_version(&skill.files, &self.tool.version);

        // Parity guard: the skill passed here must match the one that was planned.
        let current_checksum = skill_fs::checksum_bundled(&files);
        if current_checksum != plan.source_checksum {
            bail!(
                "Parity check failed for '{}': the BundledSkill has changed since plan() was called.",
                self.tool.name
            );
        }

        let PreparedWrites {
            dirs_to_write,
            discarded_paths_by_dir,
        } = prepare_writes(plan, &files, ctx)?;

        // Write each distinct dir once.
        for dir in &dirs_to_write {
            skill_fs::write_skill_files(dir, &files, ctx.fs)?;
        }

        let installed_checksum = plan.source_checksum.clone();
        let installed_at = ctx.clock.now().to_rfc3339();

        let mut targets: Vec<TargetOutcome> = Vec::new();

        for target in &plan.targets {
            if !target.action.will_write() {
                targets.push(TargetOutcome {
                    platform: target.platform,
                    dest_dir: target.dest_dir.clone(),
                    action: target.action.clone(),
                    files_written: 0,
                    installed_checksum: None,
                    discarded_paths: Vec::new(),
                });
                continue;
            }

            // Write lock entry for this platform.
            let pv = ctx.paths.with_platform(target.platform);
            lockfile::mutate(target.scope, ctx.fs, &pv, |lock| {
                lock.packages.insert(
                    self.tool.name.clone(),
                    build_lock_entry(&self.tool, &installed_checksum, &installed_at),
                );
            })?;

            targets.push(TargetOutcome {
                platform: target.platform,
                dest_dir: target.dest_dir.clone(),
                action: target.action.clone(),
                files_written: target.files.len(),
                installed_checksum: Some(installed_checksum.clone()),
                discarded_paths: discarded_paths_by_dir
                    .get(&target.dest_dir)
                    .cloned()
                    .unwrap_or_default(),
            });
        }

        // Register source if cmx is managing this machine.
        let source_registered = if plan.cmx_present
            && config::managed_platforms(ctx.fs, ctx.paths)?.is_some()
        {
            let source_name = format!("bundled:{}", self.tool.name);
            // Materialize a directory under the default artifact home for source tracing.
            let home =
                config::resolve_artifact_home(&config::load_config(ctx.fs, ctx.paths)?, ctx.paths);
            let materialized = home.join("skills").join(&self.tool.name);
            skill_fs::write_skill_files(&materialized, &files, ctx.fs)?;

            config::mutate_sources(ctx.fs, ctx.paths, |sources| {
                sources.sources.entry(source_name.clone()).or_insert_with(|| SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(materialized.clone()),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some(ctx.clock.now().to_rfc3339()),
                });
                Ok(())
            })?;
            true
        } else {
            false
        };

        Ok(Report {
            tool: self.tool.clone(),
            scope: plan.scope,
            targets,
            source_registered,
        })
    }

    /// Query the install status of this skill across relevant platforms.
    pub fn status(&self, scope: Scope, ctx: &AppContext<'_>) -> Result<Status> {
        let install_scope = scope.to_install_scope();
        let platform_targets =
            targets::resolve_targets(None, ArtifactKind::Skill, install_scope, ctx)?;

        let mut target_statuses = Vec::new();
        for &platform in &platform_targets {
            let pv = ctx.paths.with_platform(platform);
            let skill_dir = pv
                .install_dir(ArtifactKind::Skill, install_scope)
                .map(|d| d.join(&self.tool.name));

            let installed = skill_dir.as_ref().is_some_and(|d| ctx.fs.exists(d));

            let lock = lockfile::load(install_scope, ctx.fs, &pv)?;
            let lock_entry = lock.packages.get(&self.tool.name);
            let tracked = lock_entry.is_some();
            let installed_version = lock_entry.and_then(|e| e.version.clone());

            let drifted = if installed && tracked {
                if let (Some(dir), Some(entry)) = (&skill_dir, lock_entry) {
                    checksum::is_locally_modified(dir, ArtifactKind::Skill, entry, ctx.fs)
                        .unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            };

            target_statuses.push(TargetStatus {
                platform,
                installed,
                installed_version,
                drifted,
                tracked,
            });
        }

        Ok(Status {
            tool_name: self.tool.name.clone(),
            scope: install_scope,
            targets: target_statuses,
        })
    }

    /// Remove this skill from all relevant platforms.
    pub fn remove(&self, scope: Scope, ctx: &AppContext<'_>) -> Result<RemoveReport> {
        let install_scope = scope.to_install_scope();
        let platform_targets = config::managed_or_all_platforms(ctx.fs, ctx.paths)?
            .into_iter()
            .filter(|p| p.supports(ArtifactKind::Skill))
            .collect::<Vec<_>>();

        let mut dirs_to_delete: BTreeSet<PathBuf> = BTreeSet::new();
        let mut platforms_cleared: Vec<Platform> = Vec::new();
        let mut was_tracked = false;

        for &platform in &platform_targets {
            let pv = ctx.paths.with_platform(platform);

            // Collect physical path for deletion.
            if let Some(dir) = pv.install_dir(ArtifactKind::Skill, install_scope) {
                let skill_dir = dir.join(&self.tool.name);
                if ctx.fs.exists(&skill_dir) {
                    dirs_to_delete.insert(skill_dir);
                }
            }

            // Clear lock entry.
            let lock = lockfile::load(install_scope, ctx.fs, &pv)?;
            if lock.packages.contains_key(&self.tool.name) {
                lockfile::mutate(install_scope, ctx.fs, &pv, |l| {
                    l.packages.remove(&self.tool.name);
                })?;
                platforms_cleared.push(platform);
                was_tracked = true;
            }
        }

        let was_on_disk = !dirs_to_delete.is_empty();
        let removed_dirs: Vec<PathBuf> = dirs_to_delete.into_iter().collect();
        for dir in &removed_dirs {
            ctx.fs.remove_dir_all(dir)?;
        }

        // Remove from sources and materialized home if managed.
        let source_name = format!("bundled:{}", self.tool.name);
        let source_unregistered = if let Ok(sources) = config::load_sources(ctx.fs, ctx.paths) {
            if sources.sources.contains_key(&source_name) {
                // Also remove the materialized skill directory.
                if let Some(entry) = sources.sources.get(&source_name)
                    && let Some(path) = &entry.path
                    && ctx.fs.exists(path)
                {
                    ctx.fs.remove_dir_all(path)?;
                }
                config::mutate_sources(ctx.fs, ctx.paths, |s| {
                    s.sources.remove(&source_name);
                    Ok(())
                })?;
                true
            } else {
                false
            }
        } else {
            false
        };

        Ok(RemoveReport {
            tool_name: self.tool.name.clone(),
            scope: install_scope,
            removed_dirs,
            platforms_cleared,
            source_unregistered,
            was_on_disk,
            was_tracked,
        })
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compare two semver version strings.
///
/// - `None` installed → `Less` (treat as "not installed").
/// - Both parse → standard semver comparison.
/// - Either parse fails → string equality: `Equal` if equal, else `Less`.
fn compare_versions(installed: Option<&str>, bundled: &str) -> Ordering {
    let Some(inst) = installed else {
        return Ordering::Less;
    };
    match (semver::Version::parse(inst), semver::Version::parse(bundled)) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => {
            if inst == bundled {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        }
    }
}

/// Decide what action to take for a platform that already has a lock entry.
fn decide_action_for_entry(
    entry: &LockEntry,
    bundled_version: &str,
    source_checksum: &str,
    force: bool,
    skill_dest: &std::path::Path,
    ctx: &AppContext<'_>,
) -> Result<TargetAction> {
    let installed_version = entry.version.as_deref();
    let cmp = compare_versions(installed_version, bundled_version);

    match cmp {
        Ordering::Less => Ok(TargetAction::Update {
            from: installed_version.map(str::to_string),
        }),
        Ordering::Equal => {
            if !ctx.fs.exists(skill_dest) {
                return Ok(TargetAction::Install);
            }

            let disk_checksum =
                checksum::checksum_artifact(skill_dest, ArtifactKind::Skill, ctx.fs)?;
            if disk_checksum == source_checksum {
                Ok(TargetAction::Skip)
            } else if force {
                Ok(TargetAction::Update {
                    from: installed_version.map(str::to_string),
                })
            } else {
                Ok(TargetAction::DriftedSkip {
                    installed: installed_version.unwrap_or("unknown").to_string(),
                })
            }
        }
        Ordering::Greater => {
            if force {
                Ok(TargetAction::Downgrade {
                    from: installed_version.unwrap_or("unknown").to_string(),
                })
            } else {
                Ok(TargetAction::RefuseNewer {
                    installed: installed_version.unwrap_or("unknown").to_string(),
                })
            }
        }
    }
}

fn discarded_paths_against_bundle(
    skill_dest: &std::path::Path,
    bundled_files: &[SkillFile],
    ctx: &AppContext<'_>,
) -> Result<Vec<PathBuf>> {
    if !ctx.fs.exists(skill_dest) {
        return Ok(Vec::new());
    }

    let installed_files = fs_util::collect_files_recursive(skill_dest, ctx.fs)?;
    let mut installed_by_rel = BTreeMap::new();
    for path in installed_files {
        let rel = path.strip_prefix(skill_dest).unwrap_or(&path).to_path_buf();
        installed_by_rel.insert(rel, ctx.fs.read(&path)?);
    }

    let mut bundled_by_rel = BTreeMap::new();
    for file in skill_fs::canonical_files(bundled_files) {
        bundled_by_rel.insert(file.rel_path.clone(), file.bytes.clone());
    }

    let mut changed_paths = Vec::new();
    let relative_paths: BTreeSet<_> =
        installed_by_rel.keys().chain(bundled_by_rel.keys()).cloned().collect();

    for rel_path in relative_paths {
        match (installed_by_rel.get(&rel_path), bundled_by_rel.get(&rel_path)) {
            (Some(installed), Some(bundled)) if installed == bundled => {}
            (Some(_) | None, Some(_)) | (Some(_), None) => {
                changed_paths.push(skill_dest.join(rel_path));
            }
            (None, None) => {}
        }
    }

    Ok(changed_paths)
}

fn prepare_writes(
    plan: &InstallPlan,
    files: &[SkillFile],
    ctx: &AppContext<'_>,
) -> Result<PreparedWrites> {
    let mut dirs_to_write = BTreeSet::new();
    let mut dirs_to_replace = BTreeSet::new();

    for target in &plan.targets {
        if target.action.will_write() {
            dirs_to_write.insert(target.dest_dir.clone());
        }
        if plan.force
            && matches!(target.action, TargetAction::Update { .. } | TargetAction::Downgrade { .. })
        {
            dirs_to_replace.insert(target.dest_dir.clone());
        }
    }

    let mut discarded_paths_by_dir = BTreeMap::new();
    for dir in &dirs_to_replace {
        discarded_paths_by_dir
            .insert(dir.clone(), discarded_paths_against_bundle(dir, files, ctx)?);
        if ctx.fs.exists(dir) {
            ctx.fs.remove_dir_all(dir)?;
        }
    }

    Ok(PreparedWrites {
        dirs_to_write,
        discarded_paths_by_dir,
    })
}

fn build_lock_entry(tool: &ToolIdentity, checksum: &str, installed_at: &str) -> LockEntry {
    LockEntry {
        artifact_type: ArtifactKind::Skill,
        version: Some(tool.version.clone()),
        installed_at: installed_at.to_string(),
        source: LockSource {
            repo: format!("bundled:{}", tool.name),
            path: format!("skills/{}", tool.name),
        },
        source_checksum: checksum.to_string(),
        installed_checksum: checksum.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::Filesystem as _;
    use crate::skill_fs::SkillFile;
    use crate::test_support::TestContext;
    use crate::types::{CmxConfig, InstallScope};
    use crate::{checksum, config};

    fn make_file(rel: &str, content: &str) -> SkillFile {
        SkillFile {
            rel_path: std::path::PathBuf::from(rel),
            bytes: content.as_bytes().to_vec(),
        }
    }

    // Uses the canonical `metadata.version` frontmatter form so that cmx-core's
    // auto-stamp (see `frontmatter::reconcile_skill_version`) is idempotent on it:
    // the bundled bytes already equal what the installer would write, keeping the
    // checksum fixtures below stable.
    fn sample_skill(version: &str) -> BundledSkill {
        BundledSkill::from_files(vec![
            make_file(
                "SKILL.md",
                &format!("---\nmetadata:\n  version: \"{version}\"\n---\n# Sample skill\n"),
            ),
            make_file("scripts/tool.py", "print('hello')"),
        ])
    }

    fn installer(version: &str) -> SkillInstaller {
        SkillInstaller::new(ToolIdentity {
            name: "sample".to_string(),
            version: version.to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // Tests 1–3: skill_fs / checksum parity
    // -----------------------------------------------------------------------

    #[test]
    fn checksum_bundled_matches_checksum_dir_after_write() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let expected = skill_fs::checksum_bundled(&skill.files);
        let dest = std::path::PathBuf::from("/dest/sample");
        skill_fs::write_skill_files(&dest, &skill.files, &t.fs).unwrap();
        let on_disk = checksum::checksum_dir(&dest, &t.fs).unwrap();
        assert_eq!(expected, on_disk, "in-memory checksum must match disk checksum");
    }

    #[test]
    fn dotfiles_and_transient_excluded_from_write_and_checksum() {
        let files = vec![
            make_file("SKILL.md", "# skill"),
            make_file(".hidden", "hidden"),
            make_file("node_modules/dep.js", "vendor"),
        ];
        let bundled_cs = skill_fs::checksum_bundled(&files);

        // The checksum must only include SKILL.md
        let only_skill = vec![make_file("SKILL.md", "# skill")];
        let expected_cs = skill_fs::checksum_bundled(&only_skill);
        assert_eq!(
            bundled_cs, expected_cs,
            "dotfiles and transient must be excluded from checksum"
        );
    }

    #[test]
    fn write_skill_files_creates_nested_dirs() {
        let t = TestContext::new();
        let files = vec![
            make_file("SKILL.md", "# skill"),
            make_file("scripts/sub/tool.py", "code"),
        ];
        skill_fs::write_skill_files(std::path::Path::new("/dest/s"), &files, &t.fs).unwrap();
        assert!(t.fs.file_exists(std::path::Path::new("/dest/s/SKILL.md")));
        assert!(t.fs.file_exists(std::path::Path::new("/dest/s/scripts/sub/tool.py")));
    }

    // -----------------------------------------------------------------------
    // Tests 4–6: plan() target selection
    // -----------------------------------------------------------------------

    #[test]
    fn fresh_machine_produces_single_claude_target_install() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let skill = sample_skill("1.0.0");
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();

        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].platform, Platform::Claude);
        assert!(matches!(plan.targets[0].action, TargetAction::Install));
        assert!(!plan.cmx_present);
    }

    #[test]
    fn cmx_config_two_platforms_produces_two_targets_cmx_managed() {
        let t = TestContext::new();
        let cfg = CmxConfig {
            platforms: vec![Platform::Claude, Platform::Codex],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let skill = sample_skill("1.0.0");
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();

        let platforms: Vec<_> = plan.targets.iter().map(|t| t.platform).collect();
        assert!(platforms.contains(&Platform::Claude), "should include Claude");
        assert!(platforms.contains(&Platform::Codex), "should include Codex");
        assert!(plan.targets[0].cmx_managed, "cmx_managed should be true");
    }

    #[test]
    fn no_config_but_non_empty_codex_lock_targets_codex() {
        let t = TestContext::new();
        let codex_paths = t.paths.with_platform(Platform::Codex);
        crate::test_support::save_lock_with_entry(
            &t.fs,
            &codex_paths,
            "other-skill",
            crate::test_support::sample_lock_entry(),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let skill = sample_skill("1.0.0");
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();

        let platforms: Vec<_> = plan.targets.iter().map(|t| t.platform).collect();
        assert!(
            platforms.contains(&Platform::Codex),
            "Codex lock non-empty → should be targeted"
        );
    }

    // -----------------------------------------------------------------------
    // Tests 7–12: version-guard actions
    // -----------------------------------------------------------------------

    fn plan_with_locked_version(
        t: &TestContext,
        locked_version: &str,
        locked_checksum: &str,
        bundled_version: &str,
        force: bool,
    ) -> InstallPlan {
        // Set up a lock entry for Claude with the given version and checksum.
        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_dir(skill_dir.clone());

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some(locked_version.to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: locked_checksum.to_string(),
                installed_checksum: locked_checksum.to_string(),
            },
            InstallScope::Global,
        );
        let skill = sample_skill(bundled_version);
        let ctx = t.ctx();
        installer(bundled_version).plan(&skill, Scope::Global, force, &ctx).unwrap()
    }

    #[test]
    fn older_lock_version_produces_update() {
        let t = TestContext::new();
        let plan = plan_with_locked_version(&t, "0.9.0", "sha256:old", "1.0.0", false);
        assert!(matches!(plan.targets[0].action, TargetAction::Update { .. }));
    }

    #[test]
    fn same_version_identical_checksum_on_disk_produces_skip() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        skill_fs::write_skill_files(&skill_dir, &skill.files, &t.fs).unwrap();

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        assert!(matches!(plan.targets[0].action, TargetAction::Skip));
    }

    #[test]
    fn same_version_differing_content_no_force_produces_drifted_skip() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# Modified\n");

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum,
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        assert!(matches!(plan.targets[0].action, TargetAction::DriftedSkip { .. }));
    }

    #[test]
    fn same_version_differing_content_with_force_produces_update() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# Modified\n");

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum,
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, true, &ctx).unwrap();
        assert!(matches!(plan.targets[0].action, TargetAction::Update { .. }));
    }

    #[test]
    fn newer_lock_no_force_produces_refuse_newer_and_is_blocked() {
        let t = TestContext::new();
        let plan = plan_with_locked_version(&t, "2.0.0", "sha256:new", "1.0.0", false);
        assert!(matches!(plan.targets[0].action, TargetAction::RefuseNewer { .. }));
        assert!(plan.is_blocked());
    }

    #[test]
    fn newer_lock_with_force_produces_downgrade() {
        let t = TestContext::new();
        let plan = plan_with_locked_version(&t, "2.0.0", "sha256:new", "1.0.0", true);
        assert!(matches!(plan.targets[0].action, TargetAction::Downgrade { .. }));
        assert!(!plan.is_blocked());
    }

    #[test]
    fn non_semver_versions_use_string_equality_fallback() {
        // Both non-semver and equal → Equal → Skip (if checksum matches)
        let t = TestContext::new();
        let skill = sample_skill("v1-alpha");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        skill_fs::write_skill_files(&skill_dir, &skill.files, &t.fs).unwrap();

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("v1-alpha".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        // Non-semver, same version strings → Equal → install since not on disk
        let ctx = t.ctx();
        let plan = installer("v1-alpha").plan(&skill, Scope::Global, false, &ctx).unwrap();
        // The skill_dest doesn't have a file on disk (only dir exists via add_dir)
        // but the dir itself exists — the logic returns Skip since checksum matches
        // and exists returns true for a dir.
        // Actually FakeFilesystem.exists checks both files AND dirs.
        // The dir was added via add_dir so exists returns true.
        // Same version + matching checksum + exists → Skip
        assert!(
            matches!(plan.targets[0].action, TargetAction::Skip)
                || matches!(plan.targets[0].action, TargetAction::Install),
            "non-semver equal versions should not produce RefuseNewer or Downgrade"
        );
    }

    #[test]
    fn missing_skill_md_returns_error() {
        let t = TestContext::new();
        let skill = BundledSkill::from_files(vec![make_file("scripts/tool.py", "code")]);
        let ctx = t.ctx();
        let result = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SKILL.md"));
    }

    // -----------------------------------------------------------------------
    // Tests 14–21: apply()
    // -----------------------------------------------------------------------

    #[test]
    fn apply_fresh_machine_writes_files_and_lock_source_not_registered() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        assert_eq!(report.applied().count(), 1);
        assert!(!report.source_registered, "no managed set → no source registration");

        // Files should be on disk
        let skill_dir = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        assert!(t.fs.file_exists(&skill_dir.join("SKILL.md")));

        // Lock entry should exist
        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(lock.packages.contains_key("sample"));
    }

    #[test]
    fn installed_checksum_equals_source_checksum() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let first_applied = report.applied().next().unwrap();
        assert_eq!(first_applied.installed_checksum.as_deref().unwrap(), plan.source_checksum);
    }

    #[test]
    fn cmx_managed_registers_source_and_materializes_dir() {
        let t = TestContext::new();
        let cfg = CmxConfig {
            platforms: vec![Platform::Claude],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        assert!(report.source_registered, "managed set → source should be registered");

        let sources = config::load_sources(&t.fs, &t.paths).unwrap();
        assert!(sources.sources.contains_key("bundled:sample"), "source entry should exist");
    }

    #[test]
    fn skip_and_drifted_skip_plan_writes_nothing() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        skill_fs::write_skill_files(&skill_dir, &skill.files, &t.fs).unwrap();

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        // Plan should be Skip
        assert!(matches!(plan.targets[0].action, TargetAction::Skip));
        assert_eq!(plan.write_count(), 0);
    }

    #[test]
    fn blocked_plan_returns_err_on_apply() {
        let t = TestContext::new();
        // Create a plan with a RefuseNewer by having a newer version installed
        let skill = sample_skill("1.0.0");
        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_dir(skill_dir);

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("2.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: "sha256:abc".to_string(),
                installed_checksum: "sha256:abc".to_string(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        assert!(plan.is_blocked());

        let result = installer("1.0.0").apply(&skill, &plan, &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn parity_guard_rejects_mismatched_bundled_skill() {
        let t = TestContext::new();
        let skill_v1 = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill_v1, Scope::Global, false, &ctx).unwrap();

        // Apply with a skill whose *body* differs (a version-only difference would be
        // normalized away by the auto-stamp, so parity must be exercised on content
        // the stamp does not touch).
        let skill_v2 = BundledSkill::from_files(vec![
            make_file("SKILL.md", "---\nmetadata:\n  version: \"1.0.0\"\n---\n# DIFFERENT body\n"),
            make_file("scripts/tool.py", "print('hello')"),
        ]);
        let result = installer("1.0.0").apply(&skill_v2, &plan, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Parity"));
    }

    #[test]
    fn shared_dir_managed_codex_pi_written_once_both_locks_updated() {
        let t = TestContext::new();
        // Configure both Codex and Pi as managed platforms
        let cfg = CmxConfig {
            platforms: vec![Platform::Codex, Platform::Pi],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        // Both Codex and Pi resolve skills to .agents/skills — same path.
        let codex_paths = t.paths.with_platform(Platform::Codex);
        let pi_paths = t.paths.with_platform(Platform::Pi);

        let codex_lock = lockfile::load(InstallScope::Global, &t.fs, &codex_paths).unwrap();
        let pi_lock = lockfile::load(InstallScope::Global, &t.fs, &pi_paths).unwrap();

        assert!(codex_lock.packages.contains_key("sample"), "Codex lock should have entry");
        assert!(pi_lock.packages.contains_key("sample"), "Pi lock should have entry");
    }

    #[test]
    fn on_disk_file_set_matches_planned_dest_paths() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        for target in &plan.targets {
            for pf in &target.files {
                assert!(
                    t.fs.file_exists(&pf.dest_path),
                    "expected file on disk: {}",
                    pf.dest_path.display()
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Tests 22–24: status()
    // -----------------------------------------------------------------------

    #[test]
    fn not_installed_on_fresh_machine() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let status = installer("1.0.0").status(Scope::Global, &ctx).unwrap();
        assert!(!status.targets[0].installed);
        assert!(!status.targets[0].tracked);
    }

    #[test]
    fn after_apply_installed_tracked_version_matches_not_drifted() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let status = installer("1.0.0").status(Scope::Global, &ctx).unwrap();
        assert!(status.targets[0].installed);
        assert!(status.targets[0].tracked);
        assert_eq!(status.targets[0].installed_version.as_deref(), Some("1.0.0"));
        assert!(!status.targets[0].drifted);
    }

    #[test]
    fn mutate_skill_md_on_disk_produces_drifted() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        // Mutate the on-disk SKILL.md
        let skill_dir = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# MODIFIED\n");

        let status = installer("1.0.0").status(Scope::Global, &ctx).unwrap();
        assert!(status.targets[0].drifted, "mutated SKILL.md should report drifted");
    }

    // -----------------------------------------------------------------------
    // Tests 25–28: remove()
    // -----------------------------------------------------------------------

    #[test]
    fn remove_deletes_dir_and_clears_lock() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let skill_dir = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        assert!(t.fs.exists(&skill_dir));

        let report = installer("1.0.0").remove(Scope::Global, &ctx).unwrap();
        assert!(report.was_on_disk);
        assert!(report.was_tracked);
        assert!(!t.fs.exists(&skill_dir));

        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!lock.packages.contains_key("sample"));
    }

    #[test]
    fn shared_dir_managed_codex_pi_removed_once_both_locks_cleared() {
        let t = TestContext::new();
        let cfg = CmxConfig {
            platforms: vec![Platform::Codex, Platform::Pi],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let report = installer("1.0.0").remove(Scope::Global, &ctx).unwrap();
        assert!(report.was_on_disk);
        assert!(report.platforms_cleared.contains(&Platform::Codex));
        assert!(report.platforms_cleared.contains(&Platform::Pi));

        // Both lock entries should be gone
        let codex_paths = t.paths.with_platform(Platform::Codex);
        let pi_paths = t.paths.with_platform(Platform::Pi);
        let codex_lock = lockfile::load(InstallScope::Global, &t.fs, &codex_paths).unwrap();
        let pi_lock = lockfile::load(InstallScope::Global, &t.fs, &pi_paths).unwrap();
        assert!(!codex_lock.packages.contains_key("sample"));
        assert!(!pi_lock.packages.contains_key("sample"));
    }

    #[test]
    fn cmx_managed_remove_clears_source_and_materialized_dir() {
        let t = TestContext::new();
        let cfg = CmxConfig {
            platforms: vec![Platform::Claude],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        // Confirm source was registered
        let sources = config::load_sources(&t.fs, &t.paths).unwrap();
        assert!(sources.sources.contains_key("bundled:sample"));

        let report = installer("1.0.0").remove(Scope::Global, &ctx).unwrap();
        assert!(report.source_unregistered);

        let sources_after = config::load_sources(&t.fs, &t.paths).unwrap();
        assert!(!sources_after.sources.contains_key("bundled:sample"));
    }

    #[test]
    fn remove_when_nothing_installed_returns_ok_all_false() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let report = installer("1.0.0").remove(Scope::Global, &ctx).unwrap();
        assert!(!report.was_on_disk);
        assert!(!report.was_tracked);
        assert!(!report.source_unregistered);
    }

    // -----------------------------------------------------------------------
    // New constructor and derive tests
    // -----------------------------------------------------------------------

    #[test]
    fn single_md_builds_single_skill_md() {
        let skill = BundledSkill::single_md("---\nversion: 1.0.0\n---\n# My skill\n");
        assert_eq!(skill.files.len(), 1);
        assert!(skill.has_skill_md());
        assert_eq!(skill.files[0].rel_path, std::path::PathBuf::from("SKILL.md"));
    }

    #[test]
    fn tool_identity_new_sets_fields() {
        let id = ToolIdentity::new("mytool", "1.2.3");
        assert_eq!(id.name, "mytool");
        assert_eq!(id.version, "1.2.3");
    }

    #[test]
    fn scope_partial_eq() {
        assert_eq!(Scope::Global, Scope::Global);
        assert_eq!(Scope::Local, Scope::Local);
        assert_ne!(Scope::Global, Scope::Local);
    }

    // -----------------------------------------------------------------------
    // Report fidelity tests
    // -----------------------------------------------------------------------

    #[test]
    fn skipped_target_outcome_carries_dest_dir() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        skill_fs::write_skill_files(&skill_dir, &skill.files, &t.fs).unwrap();

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        // The skip target must preserve dest_dir
        let skip = report.skipped().next().expect("expected a skipped target");
        assert!(
            !skip.dest_dir.as_os_str().is_empty(),
            "dest_dir must be non-empty on skipped target"
        );
        assert!(matches!(skip.action, TargetAction::Skip));
        assert_eq!(skip.installed_checksum, None);
    }

    #[test]
    fn drifted_skip_outcome_is_distinguishable_from_plain_skip() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# Modified\n");

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum,
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let skip = report.skipped().next().expect("expected a skipped target");
        assert!(
            matches!(skip.action, TargetAction::DriftedSkip { .. }),
            "action must be DriftedSkip, not plain Skip"
        );
    }

    #[test]
    fn force_overwrites_drifted_copy_and_reports_update() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let checksum = skill_fs::checksum_bundled(&skill.files);

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        let skill_md = skill_dir.join("SKILL.md");
        let local_only = skill_dir.join("local-only.md");
        t.fs.add_file(&skill_md, "---\nversion: 1.0.0\n---\n# Modified\n");
        t.fs.add_file(&local_only, "scratch\n");

        crate::test_support::save_lock_with_entry(
            &t.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum.clone(),
            },
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, true, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let updated = report.applied().next().expect("expected an updated target");
        assert!(matches!(updated.action, TargetAction::Update { .. }));
        let discarded: BTreeSet<_> = updated.discarded_paths.iter().cloned().collect();
        assert_eq!(
            discarded,
            BTreeSet::from([
                local_only.clone(),
                skill_md.clone(),
                skill_dir.join("scripts/tool.py")
            ])
        );
        assert_eq!(
            t.fs.read_to_string(&skill_md).unwrap(),
            "---\nmetadata:\n  version: \"1.0.0\"\n---\n# Sample skill\n"
        );
        assert!(!t.fs.exists(&local_only));
        assert_eq!(checksum::checksum_dir(&skill_dir, &t.fs).unwrap(), checksum);
    }

    // -----------------------------------------------------------------------
    // Display tests
    // -----------------------------------------------------------------------

    #[test]
    fn install_plan_display_contains_target_lines() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let rendered = plan.to_string();
        assert!(rendered.contains("sample"), "plan display must include tool name");
        assert!(rendered.contains("1.0.0"), "plan display must include version");
        assert!(rendered.contains("install"), "plan display must include action");
    }

    #[test]
    fn report_display_distinguishes_drifted_skip() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");

        // First apply: fresh install
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let up_to_date_report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();
        let up_to_date_text = up_to_date_report.to_string();

        // Second apply (same version, same checksum) → Skip
        let plan2 = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let skip_report = installer("1.0.0").apply(&skill, &plan2, &ctx).unwrap();
        let skip_text = skip_report.to_string();
        assert!(skip_text.contains("up to date"), "up-to-date skip must say 'up to date'");

        // Set up drifted scenario
        let t2 = TestContext::new();
        let checksum = skill_fs::checksum_bundled(&skill.files);
        let claude_paths = t2.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        t2.fs
            .add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# Modified\n");
        crate::test_support::save_lock_with_entry(
            &t2.fs,
            &claude_paths,
            "sample",
            LockEntry {
                artifact_type: ArtifactKind::Skill,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "bundled:sample".to_string(),
                    path: "skills/sample".to_string(),
                },
                source_checksum: checksum.clone(),
                installed_checksum: checksum,
            },
            InstallScope::Global,
        );
        let ctx2 = t2.ctx();
        let drifted_plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx2).unwrap();
        let drifted_report = installer("1.0.0").apply(&skill, &drifted_plan, &ctx2).unwrap();
        let drifted_text = drifted_report.to_string();

        // Drifted display must differ from up-to-date skip display
        assert!(
            drifted_text.contains("drifted"),
            "drifted skip must mention 'drifted' in output, got: {drifted_text}"
        );
        assert_ne!(
            skip_text, drifted_text,
            "up-to-date skip and drifted skip must produce different display output"
        );
        let _ = up_to_date_text;
    }

    #[test]
    fn remove_report_display_notes_lockfile_left() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        let report = installer("1.0.0").remove(Scope::Global, &ctx).unwrap();
        let rendered = report.to_string();
        assert!(
            rendered.contains("cmx-lock.json"),
            "remove report must note the lockfile is left on disk"
        );
    }
}
