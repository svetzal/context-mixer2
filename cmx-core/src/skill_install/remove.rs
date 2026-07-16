use crate::config;
use crate::context::AppContext;
use crate::error::Result;
use crate::lockfile;
use crate::platform::Platform;
use crate::types::ArtifactKind;

use super::{RemoveReport, Scope, SkillInstaller};

impl SkillInstaller {
    /// Remove this skill from all relevant platforms.
    pub fn remove(&self, scope: Scope, ctx: &AppContext<'_>) -> Result<RemoveReport> {
        let install_scope = scope.to_install_scope();
        let platform_targets = config::managed_or_all_platforms(ctx.fs, ctx.paths)?
            .into_iter()
            .filter(|p| p.supports(ArtifactKind::Skill))
            .collect::<Vec<_>>();

        let mut dirs_to_delete: std::collections::BTreeSet<std::path::PathBuf> =
            std::collections::BTreeSet::new();
        let mut platforms_cleared: Vec<Platform> = Vec::new();
        let mut was_tracked = false;

        for &platform in &platform_targets {
            let pv = ctx.paths.with_platform(platform);

            if let Some(dir) = pv.install_dir(ArtifactKind::Skill, install_scope) {
                let skill_dir = dir.join(&self.tool.name);
                if ctx.fs.exists(&skill_dir) {
                    dirs_to_delete.insert(skill_dir);
                }
            }

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
        let removed_dirs: Vec<std::path::PathBuf> = dirs_to_delete.into_iter().collect();
        for dir in &removed_dirs {
            ctx.fs.remove_dir_all(dir)?;
        }

        let source_name = format!("bundled:{}", self.tool.name);
        let source_unregistered = if let Ok(sources) = config::load_sources(ctx.fs, ctx.paths) {
            if sources.sources.contains_key(&source_name) {
                if let Some(entry) = sources.sources.get(&source_name)
                    && let Some(path) = &entry.path
                    && ctx.fs.exists(path)
                {
                    ctx.fs.remove_dir_all(path)?;
                }
                config::mutate_sources(ctx.fs, ctx.paths, |s| -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::super::test_support::{installer, sample_skill};
    use crate::config;
    use crate::gateway::filesystem::Filesystem;
    use crate::lockfile;
    use crate::platform::Platform;
    use crate::skill_install::Scope;
    use crate::test_support::TestContext;
    use crate::types::{ArtifactKind, CmxConfig, InstallScope};

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
}
