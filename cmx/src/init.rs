//! `cmx init` — install/remove cmx's own companion agent skill through
//! `cmx-core`'s embeddable [`SkillInstaller`], following the fleet-standard
//! `<tool> init` conventions (see `EMBEDDING.md`).

use anyhow::Result;
use std::process::ExitCode;

use cmx_core::context::AppContext;
use cmx_core::skill_install::{
    BundledSkill, RemoveReport, Report, Scope, SkillInstaller, TargetAction, ToolIdentity,
};

/// The companion skill bundled into the `cmx` binary. Its frontmatter carries a
/// placeholder `metadata.version` that is stamped to the cmx binary version at
/// install time — see [`stamp_version`].
const SKILL_CONTENT: &str = include_str!("../skill/SKILL.md");

/// Map the `--local` flag onto a `cmx-core` [`Scope`]. Global is the default.
fn scope_from_flags(local: bool) -> Scope {
    if local { Scope::Local } else { Scope::Global }
}

/// Stamp the cmx binary version (`CARGO_PKG_VERSION`, the workspace version)
/// into the bundled skill's frontmatter `metadata.version`, locking the skill's
/// declared version to the cmx release rather than hand-maintaining it.
///
/// cmx-core already tracks the install by the same `CARGO_PKG_VERSION` (via
/// [`ToolIdentity`]); the frontmatter is *also* read when the installed skill is
/// scanned as a source artifact (`scan::frontmatter`), so leaving it out of step
/// produces an internal version inconsistency, not merely a cosmetic one. This
/// keeps the single source of truth the workspace `Cargo.toml` version.
///
/// Replaces the first indented `version:` line inside the leading `---` fenced
/// frontmatter block, preserving its indentation.
fn stamp_version(content: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let mut out = String::with_capacity(content.len() + 8);
    let mut fences = 0u8;
    let mut stamped = false;
    for line in content.lines() {
        if line.trim() == "---" {
            fences += 1;
        } else if fences == 1 && !stamped {
            let trimmed = line.trim_start();
            if trimmed.starts_with("version:") {
                let indent = &line[..line.len() - trimmed.len()];
                out.push_str(indent);
                out.push_str("version: \"");
                out.push_str(version);
                out.push_str("\"\n");
                stamped = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn bundled_skill() -> BundledSkill {
    BundledSkill::single_md(&stamp_version(SKILL_CONTENT))
}

fn make_installer() -> SkillInstaller {
    SkillInstaller::new(ToolIdentity::new("cmx", env!("CARGO_PKG_VERSION")))
}

/// Outcome of `cmx init` / `cmx init --remove`, wrapping the `cmx-core` report
/// types so `Display` can delegate to them directly (see `display/init.rs`).
pub enum InitOutcome {
    Installed(Report),
    Removed(RemoveReport),
    /// The plan was blocked (e.g. a newer version is already installed and
    /// `--force` was not passed).
    Blocked {
        reasons: Vec<String>,
    },
}

impl InitOutcome {
    /// The process exit code for this outcome: non-zero only when blocked.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            InitOutcome::Blocked { .. } => ExitCode::FAILURE,
            InitOutcome::Installed(_) | InitOutcome::Removed(_) => ExitCode::SUCCESS,
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
        return Ok(InitOutcome::Blocked { reasons });
    }

    let report = installer.apply(&skill, &plan, ctx)?;
    Ok(InitOutcome::Installed(report))
}

/// Uninstall cmx's companion skill.
pub fn run_remove(local: bool, ctx: &AppContext<'_>) -> Result<InitOutcome> {
    let installer = make_installer();
    let scope = scope_from_flags(local);
    let report = installer.remove(scope, ctx)?;
    Ok(InitOutcome::Removed(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::Filesystem;
    use cmx_core::platform::Platform;
    use cmx_core::test_support::{TestContext, make_lock_entry_versioned, save_lock_with_entry};
    use cmx_core::types::{ArtifactKind, InstallScope};

    #[test]
    fn stamp_version_locks_frontmatter_to_cmx_version() {
        // The bundled source carries a placeholder; stamping must replace it
        // with the cmx binary version and leave no placeholder behind.
        let stamped = stamp_version(SKILL_CONTENT);
        let expected = format!("version: \"{}\"", env!("CARGO_PKG_VERSION"));
        assert!(
            stamped.contains(&expected),
            "stamped skill should declare the cmx version ({expected})"
        );
        assert!(
            !stamped.contains("version: \"0.0.0\""),
            "placeholder version must not survive stamping"
        );
    }

    #[test]
    fn stamp_version_only_touches_frontmatter_version() {
        // Body prose mentioning "version" and the frontmatter `author` line must
        // be preserved; only the single frontmatter version line changes.
        let stamped = stamp_version(SKILL_CONTENT);
        assert!(stamped.contains("author: Stacey Vetzal"));
        assert_eq!(
            stamped.matches(&format!("version: \"{}\"", env!("CARGO_PKG_VERSION"))).count(),
            1,
            "exactly one version line should be stamped"
        );
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
            InitOutcome::Blocked { reasons } => {
                assert_eq!(reasons.len(), 1);
                assert!(reasons[0].contains("--force"));
            }
            _ => panic!("expected Blocked"),
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
