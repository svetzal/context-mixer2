use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config;
use crate::context::AppContext;
use crate::error::CmxError;
use crate::frontmatter;
use crate::lockfile;
use crate::skill_fs;
use crate::types::{SourceEntry, SourceType};

use super::plan::{build_lock_entry, prepare_writes};
use super::types::PreparedWrites;
use super::{BundledSkill, InstallPlan, Report, SkillInstaller, TargetOutcome};

impl SkillInstaller {
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
    ) -> anyhow::Result<Report> {
        if plan.is_blocked() {
            return Err(CmxError::VersionGuard {
                tool: self.tool.name.clone(),
            }
            .into());
        }

        let files = frontmatter::reconcile_skill_version(&skill.files, &self.tool.version);

        let current_checksum = skill_fs::checksum_bundled(&files);
        if current_checksum != plan.source_checksum {
            return Err(CmxError::Drift {
                tool: self.tool.name.clone(),
            }
            .into());
        }

        let PreparedWrites {
            dirs_to_write,
            discarded_paths_by_dir,
        } = prepare_writes(plan, &files, ctx)?;

        for dir in &dirs_to_write {
            skill_fs::write_skill_files(dir, &files, ctx.fs)?;
        }

        let installed_checksum = plan.source_checksum.clone();
        let installed_at = ctx.clock.now().to_rfc3339();

        let targets = self.write_target_outcomes(
            plan,
            &discarded_paths_by_dir,
            &installed_checksum,
            &installed_at,
            ctx,
        )?;
        let source_registered = self.register_bundled_source(plan, &files, ctx)?;

        Ok(Report {
            tool: self.tool.clone(),
            scope: plan.scope,
            targets,
            source_registered,
        })
    }

    /// Write lock entries for every target that will write files and collect
    /// the per-target outcomes (both written and skipped).
    fn write_target_outcomes(
        &self,
        plan: &InstallPlan,
        discarded_paths_by_dir: &BTreeMap<PathBuf, Vec<PathBuf>>,
        installed_checksum: &str,
        installed_at: &str,
        ctx: &AppContext<'_>,
    ) -> anyhow::Result<Vec<TargetOutcome>> {
        let mut targets = Vec::new();
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

            let pv = ctx.paths.with_platform(target.platform);
            lockfile::mutate(target.scope, ctx.fs, &pv, |lock| {
                lock.packages.insert(
                    self.tool.name.clone(),
                    build_lock_entry(&self.tool, installed_checksum, installed_at),
                );
            })?;

            targets.push(TargetOutcome {
                platform: target.platform,
                dest_dir: target.dest_dir.clone(),
                action: target.action.clone(),
                files_written: target.files.len(),
                installed_checksum: Some(installed_checksum.to_string()),
                discarded_paths: discarded_paths_by_dir
                    .get(&target.dest_dir)
                    .cloned()
                    .unwrap_or_default(),
            });
        }
        Ok(targets)
    }

    /// Register a `bundled:<name>` source and materialize the home directory
    /// when cmx is managing this machine. Returns `true` when registration
    /// occurred, `false` otherwise.
    fn register_bundled_source(
        &self,
        plan: &InstallPlan,
        files: &[skill_fs::SkillFile],
        ctx: &AppContext<'_>,
    ) -> anyhow::Result<bool> {
        use crate::error::Result;
        if plan.cmx_present && config::managed_platforms(ctx.fs, ctx.paths)?.is_some() {
            let source_name = format!("bundled:{}", self.tool.name);
            let home =
                config::resolve_artifact_home(&config::load_config(ctx.fs, ctx.paths)?, ctx.paths);
            let materialized = home.join("skills").join(&self.tool.name);
            skill_fs::write_skill_files(&materialized, files, ctx.fs)?;

            config::mutate_sources(ctx.fs, ctx.paths, |sources| -> Result<()> {
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
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::test_support::{installer, make_file, sample_skill};
    use crate::gateway::filesystem::Filesystem;
    use crate::lockfile;
    use crate::platform::Platform;
    use crate::skill_fs;
    use crate::skill_install::{BundledSkill, Scope, TargetAction};
    use crate::test_support::TestContext;
    use crate::types::{ArtifactKind, CmxConfig, InstallScope, LockEntry, LockSource};
    use crate::{checksum, config};

    #[test]
    fn apply_fresh_machine_writes_files_and_lock_source_not_registered() {
        let t = TestContext::new();
        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        let report = installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

        assert_eq!(report.applied().count(), 1);
        assert!(!report.source_registered, "no managed set → no source registration");

        let skill_dir = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("sample");
        assert!(t.fs.file_exists(&skill_dir.join("SKILL.md")));

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
        assert!(matches!(plan.targets[0].action, TargetAction::Skip));
        assert_eq!(plan.write_count(), 0);
    }

    #[test]
    fn blocked_plan_returns_err_on_apply() {
        let t = TestContext::new();
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
        let cfg = CmxConfig {
            platforms: vec![Platform::Codex, Platform::Pi],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let skill = sample_skill("1.0.0");
        let ctx = t.ctx();
        let plan = installer("1.0.0").plan(&skill, Scope::Global, false, &ctx).unwrap();
        installer("1.0.0").apply(&skill, &plan, &ctx).unwrap();

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
}
