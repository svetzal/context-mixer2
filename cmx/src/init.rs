//! `cmx init` — install/remove cmx's own companion agent skill through
//! `cmx-core`'s embeddable [`SkillInstaller`], following the fleet-standard
//! `<tool> init` conventions (see `EMBEDDING.md`).

use crate::error::{CliError, Result};
use std::process::ExitCode;

use cmx_core::context::AppContext;
use cmx_core::skill_install::{
    BundledSkill, InstallPlan, RemoveReport, Report, Scope, SkillInstaller, TargetAction,
    ToolIdentity,
};

/// The companion skill bundled into the `cmx` binary. Its `metadata.version` is
/// reconciled to the cmx binary version by `cmx-core` at install time — the
/// placeholder in the file is overwritten on write, so it never needs
/// hand-maintaining here.
const SKILL_CONTENT: &str = include_str!("../skill/SKILL.md");

/// Map the `--local` flag onto a `cmx-core` [`Scope`]. Global is the default.
fn scope_from_flags(local: bool) -> Scope {
    if local { Scope::Local } else { Scope::Global }
}

fn bundled_skill() -> BundledSkill {
    BundledSkill::single_md(SKILL_CONTENT)
}

fn make_installer() -> SkillInstaller {
    SkillInstaller::new(ToolIdentity::new("cmx", env!("CARGO_PKG_VERSION")))
}

/// Outcome of `cmx init` / `cmx init --remove`.
pub enum InitOutcome {
    Installed(Report),
    Removed(RemoveReport),
    /// The plan was blocked (e.g. a newer version is already installed and
    /// `--force` was not passed).
    Blocked {
        plan: InstallPlan,
        reasons: Vec<String>,
    },
}

impl InitOutcome {
    /// The process exit code for this outcome.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            InitOutcome::Blocked { .. } => ExitCode::FAILURE,
            InitOutcome::Installed(report) => {
                let skipped_drifted = report
                    .targets
                    .iter()
                    .any(|target| matches!(target.action, TargetAction::DriftedSkip { .. }));
                if report.applied().count() == 0 && skipped_drifted {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            InitOutcome::Removed(_) => ExitCode::SUCCESS,
        }
    }
}

/// Install (or update) cmx's companion skill.
pub fn run_init(local: bool, force: bool, ctx: &AppContext<'_>) -> Result<InitOutcome> {
    let skill = bundled_skill();
    let installer = make_installer();
    let scope = scope_from_flags(local);

    let plan = installer.plan(&skill, scope, force, ctx)?;
    if plan.is_blocked() {
        let reasons = plan
            .targets
            .iter()
            .filter(|t| t.action.is_blocked())
            .map(|t| match &t.action {
                TargetAction::RefuseNewer { installed } => format!(
                    "{}: installed version {installed} is newer than the bundled version {} — use --force to override",
                    t.platform,
                    env!("CARGO_PKG_VERSION"),
                ),
                other => format!("{}: blocked ({other:?})", t.platform),
            })
            .collect();
        return Ok(InitOutcome::Blocked { plan, reasons });
    }

    let report = installer
        .apply(&skill, &plan, ctx)
        .map_err(|e| CliError::Message(e.to_string()))?;
    Ok(InitOutcome::Installed(report))
}

/// Uninstall cmx's companion skill.
pub fn run_remove(local: bool, ctx: &AppContext<'_>) -> Result<InitOutcome> {
    let installer = make_installer();
    let scope = scope_from_flags(local);
    let report = installer.remove(scope, ctx).map_err(|e| CliError::Message(e.to_string()))?;
    Ok(InitOutcome::Removed(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::checksum;
    use cmx_core::gateway::Filesystem;
    use cmx_core::platform::Platform;
    use cmx_core::test_support::{TestContext, make_lock_entry_versioned, save_lock_with_entry};
    use cmx_core::types::{ArtifactKind, InstallScope};

    /// The exact SKILL.md bytes `cmx-core` writes to disk for this build — the
    /// bundled content with `metadata.version` reconciled to the cmx version.
    fn installed_skill_md() -> String {
        let files = cmx_core::frontmatter::reconcile_skill_version(
            &bundled_skill().files,
            env!("CARGO_PKG_VERSION"),
        );
        String::from_utf8(files[0].bytes.clone()).unwrap()
    }

    #[test]
    fn install_stamps_frontmatter_to_cmx_version() {
        // cmx-core reconciles metadata.version at install; the on-disk skill must
        // declare the cmx binary version and leave no placeholder behind.
        let t = TestContext::new();
        let ctx = t.ctx();
        run_init(false, false, &ctx).unwrap();

        let skill_md = t
            .paths
            .with_platform(Platform::Claude)
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("cmx")
            .join("SKILL.md");
        let content = t.fs.read_to_string(&skill_md).unwrap();
        let expected = format!("version: \"{}\"", env!("CARGO_PKG_VERSION"));
        assert!(content.contains(&expected), "installed skill should declare {expected}");
        assert!(!content.contains("version: \"0.0.0\""), "placeholder must not survive install");
        assert!(content.contains("author: Stacey Vetzal"), "other frontmatter preserved");
    }

    #[test]
    fn scope_from_flags_maps_local_and_global() {
        assert_eq!(scope_from_flags(true), Scope::Local);
        assert_eq!(scope_from_flags(false), Scope::Global);
    }

    #[test]
    fn bundled_skill_has_skill_md() {
        assert!(bundled_skill().has_skill_md());
    }

    #[test]
    fn run_init_fresh_machine_installs() {
        let t = TestContext::new();
        let ctx = t.ctx();

        let outcome = run_init(false, false, &ctx).unwrap();
        match outcome {
            InitOutcome::Installed(report) => {
                assert_eq!(report.tool.name, "cmx");
                assert_eq!(report.applied().count(), 1);
                let target = report.targets.first().unwrap();
                assert_eq!(target.platform, Platform::Claude);
                assert!(t.fs.exists(&target.dest_dir.join("SKILL.md")));
            }
            _ => panic!("expected Installed"),
        }
    }

    #[test]
    fn run_init_rerun_is_a_skip() {
        let t = TestContext::new();
        let ctx = t.ctx();
        run_init(false, false, &ctx).unwrap();

        let outcome = run_init(false, false, &ctx).unwrap();
        match outcome {
            InitOutcome::Installed(report) => {
                assert_eq!(report.applied().count(), 0);
                assert_eq!(report.skipped().count(), 1);
                assert!(matches!(report.targets.first().unwrap().action, TargetAction::Skip));
            }
            _ => panic!("expected Installed"),
        }
    }

    #[test]
    fn run_remove_after_install_reports_removal() {
        let t = TestContext::new();
        let ctx = t.ctx();
        run_init(false, false, &ctx).unwrap();

        let outcome = run_remove(false, &ctx).unwrap();
        match outcome {
            InitOutcome::Removed(report) => {
                assert!(report.was_on_disk);
                assert!(report.was_tracked);
                assert_eq!(report.tool_name, "cmx");
            }
            _ => panic!("expected Removed"),
        }
    }

    #[test]
    fn run_init_blocked_when_installed_is_newer() {
        let t = TestContext::new();
        let entry =
            make_lock_entry_versioned(ArtifactKind::Skill, "99.0.0", "bundled:cmx", "SKILL.md");
        save_lock_with_entry(&t.fs, &t.paths, "cmx", entry, InstallScope::Global);
        let ctx = t.ctx();

        let outcome = run_init(false, false, &ctx).unwrap();
        match outcome {
            InitOutcome::Blocked { plan, reasons } => {
                assert_eq!(plan.targets.len(), 1);
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("--force"));
            }
            _ => panic!("expected Blocked"),
        }
    }

    #[test]
    fn run_init_drifted_copy_without_force_skips_and_fails() {
        let t = TestContext::new();
        let initial_ctx = t.ctx();
        run_init(false, false, &initial_ctx).unwrap();

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("cmx");
        let skill_md = skill_dir.join("SKILL.md");
        let drifted = installed_skill_md()
            .replace("## Agent contract 1", "Locally edited.\n\n## Agent contract 1");
        t.fs.add_file(&skill_md, drifted.as_str());

        let ctx = t.ctx();
        let outcome = run_init(false, false, &ctx).unwrap();
        let rendered = outcome.to_string();
        assert_eq!(outcome.exit_code(), ExitCode::FAILURE);
        assert!(rendered.contains("Skipped 1 drifted copy (use --force)."));
        assert!(rendered.contains("re-run with --force to overwrite"));
        assert!(rendered.contains("cmx skill promote cmx"));
        match outcome {
            InitOutcome::Installed(report) => {
                assert_eq!(report.applied().count(), 0);
                assert!(matches!(
                    report.targets.first().unwrap().action,
                    TargetAction::DriftedSkip { .. }
                ));
                assert_eq!(t.fs.read_to_string(&skill_md).unwrap(), drifted);
            }
            _ => panic!("expected Installed"),
        }
    }

    #[test]
    fn run_init_force_overwrites_drifted_copy_and_exits_success() {
        let t = TestContext::new();
        let initial_ctx = t.ctx();
        run_init(false, false, &initial_ctx).unwrap();

        let claude_paths = t.paths.with_platform(Platform::Claude);
        let skill_dir = claude_paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("cmx");
        let skill_md = skill_dir.join("SKILL.md");
        let local_only = skill_dir.join("local-only.md");
        t.fs.add_file(
            &skill_md,
            installed_skill_md()
                .replace("## Agent contract 1", "Locally edited.\n\n## Agent contract 1"),
        );
        t.fs.add_file(&local_only, "scratch notes\n");

        let ctx = t.ctx();
        let outcome = run_init(false, true, &ctx).unwrap();
        let rendered = outcome.to_string();
        assert_eq!(outcome.exit_code(), ExitCode::SUCCESS);
        assert!(rendered.contains("Discarding local modification:"));
        assert!(rendered.contains(&skill_md.display().to_string()));
        assert!(rendered.contains(&local_only.display().to_string()));
        match outcome {
            InitOutcome::Installed(report) => {
                assert_eq!(report.applied().count(), 1);
                assert!(matches!(
                    report.targets.first().unwrap().action,
                    TargetAction::Update { .. }
                ));
                assert_eq!(report.targets.first().unwrap().discarded_paths.len(), 2);
                assert_eq!(t.fs.read_to_string(&skill_md).unwrap(), installed_skill_md());
                assert!(!t.fs.exists(&local_only));
                let installed = cmx_core::frontmatter::reconcile_skill_version(
                    &bundled_skill().files,
                    env!("CARGO_PKG_VERSION"),
                );
                assert_eq!(
                    checksum::checksum_artifact(&skill_dir, ArtifactKind::Skill, &t.fs).unwrap(),
                    cmx_core::skill_fs::checksum_bundled(&installed)
                );
            }
            _ => panic!("expected Installed"),
        }
    }

    #[test]
    fn run_init_force_downgrades_and_exits_success() {
        let t = TestContext::new();
        let entry =
            make_lock_entry_versioned(ArtifactKind::Skill, "99.0.0", "bundled:cmx", "SKILL.md");
        save_lock_with_entry(&t.fs, &t.paths, "cmx", entry, InstallScope::Global);
        let ctx = t.ctx();

        let outcome = run_init(false, true, &ctx).unwrap();
        assert_eq!(outcome.exit_code(), ExitCode::SUCCESS);
        match outcome {
            InitOutcome::Installed(report) => {
                assert!(matches!(
                    report.targets.first().unwrap().action,
                    TargetAction::Downgrade { .. }
                ));
            }
            _ => panic!("expected Installed"),
        }
    }
}
