use crate::checksum;
use crate::context::AppContext;
use crate::error::Result;
use crate::lockfile;
use crate::targets;
use crate::types::ArtifactKind;

use super::{Scope, SkillInstaller, Status, TargetStatus};

impl SkillInstaller {
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
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{installer, sample_skill};
    use crate::skill_install::Scope;
    use crate::test_support::TestContext;
    use crate::types::ArtifactKind;

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

        let skill_dir = t
            .paths
            .install_dir(ArtifactKind::Skill, crate::types::InstallScope::Global)
            .unwrap()
            .join("sample");
        t.fs.add_file(skill_dir.join("SKILL.md"), "---\nversion: 1.0.0\n---\n# MODIFIED\n");

        let status = installer("1.0.0").status(Scope::Global, &ctx).unwrap();
        assert!(status.targets[0].drifted, "mutated SKILL.md should report drifted");
    }
}
