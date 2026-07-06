use super::{InstallPlan, RemoveReport, Report, TargetAction};

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
// Display tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::{ArtifactKind, BundledSkill, Platform, Scope, SkillInstaller, ToolIdentity};
    use crate::skill_fs;
    use crate::skill_fs::SkillFile;
    use crate::test_support::TestContext;
    use crate::types::{InstallScope, LockEntry, LockSource};

    fn make_file(rel: &str, content: &str) -> SkillFile {
        SkillFile {
            rel_path: std::path::PathBuf::from(rel),
            bytes: content.as_bytes().to_vec(),
        }
    }

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
