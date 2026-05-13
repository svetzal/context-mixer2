use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::platform::Platform;
use crate::types::ArtifactKind;

/// Centralizes all path resolution for cmx configuration and install directories.
///
/// Production code constructs this via [`ConfigPaths::from_env`]; tests use
/// [`ConfigPaths::for_test`] to inject arbitrary root directories and avoid
/// touching the real home directory.
pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub home_dir: PathBuf,
    pub platform: Platform,
}

impl ConfigPaths {
    /// Production constructor — derives paths from the real home and config directories.
    pub fn from_env(platform: Platform) -> Result<Self> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let config_dir = home.join(".config").join("context-mixer");
        Ok(Self {
            config_dir,
            home_dir: home,
            platform,
        })
    }

    /// Test constructor — uses arbitrary root directories so no real home
    /// directory is touched. Defaults to `Platform::Claude`.
    pub fn for_test(home: PathBuf, config: PathBuf) -> Self {
        Self {
            config_dir: config,
            home_dir: home,
            platform: Platform::Claude,
        }
    }

    /// Test constructor with explicit platform.
    pub fn for_test_with_platform(home: PathBuf, config: PathBuf, platform: Platform) -> Self {
        Self {
            config_dir: config,
            home_dir: home,
            platform,
        }
    }

    /// Path to `sources.json`.
    pub fn sources_path(&self) -> PathBuf {
        self.config_dir.join("sources.json")
    }

    /// Directory where git-backed sources are cloned.
    pub fn git_clones_dir(&self) -> PathBuf {
        self.config_dir.join("sources")
    }

    /// Path to `config.json` (LLM gateway settings).
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    /// Path to the lock file for the given scope.
    ///
    /// Claude uses `cmx-lock.json` for backward compatibility. All other
    /// platforms use `cmx-lock-<slug>.json`.
    pub fn lock_path(&self, local: bool) -> PathBuf {
        let file_name = if self.platform.slug().is_empty() {
            "cmx-lock.json".to_string()
        } else {
            format!("cmx-lock-{}.json", self.platform.slug())
        };

        if local {
            PathBuf::from(".context-mixer").join(&file_name)
        } else {
            self.config_dir.join(&file_name)
        }
    }

    /// Directory where artifacts of the given kind and scope are installed.
    pub fn install_dir(&self, kind: ArtifactKind, local: bool) -> PathBuf {
        let subdir = kind.subdir_name();
        if local {
            self.platform.project_base().join(subdir)
        } else {
            self.home_dir.join(self.platform.user_base()).join(subdir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> ConfigPaths {
        ConfigPaths::for_test(
            PathBuf::from("/home/testuser"),
            PathBuf::from("/home/testuser/.config/context-mixer"),
        )
    }

    fn test_paths_for(platform: Platform) -> ConfigPaths {
        ConfigPaths::for_test_with_platform(
            PathBuf::from("/home/testuser"),
            PathBuf::from("/home/testuser/.config/context-mixer"),
            platform,
        )
    }

    // --- Claude (default) ---

    #[test]
    fn sources_path_returns_config_dir_sources_json() {
        let paths = test_paths();
        assert_eq!(
            paths.sources_path(),
            PathBuf::from("/home/testuser/.config/context-mixer/sources.json")
        );
    }

    #[test]
    fn git_clones_dir_returns_config_dir_sources() {
        let paths = test_paths();
        assert_eq!(
            paths.git_clones_dir(),
            PathBuf::from("/home/testuser/.config/context-mixer/sources")
        );
    }

    #[test]
    fn config_path_returns_config_dir_config_json() {
        let paths = test_paths();
        assert_eq!(
            paths.config_path(),
            PathBuf::from("/home/testuser/.config/context-mixer/config.json")
        );
    }

    #[test]
    fn lock_path_global_returns_config_dir_lock_file() {
        let paths = test_paths();
        assert_eq!(
            paths.lock_path(false),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock.json")
        );
    }

    #[test]
    fn lock_path_local_returns_relative_path() {
        let paths = test_paths();
        assert_eq!(paths.lock_path(true), PathBuf::from(".context-mixer/cmx-lock.json"));
    }

    #[test]
    fn install_dir_agent_global_returns_home_claude_agents() {
        let paths = test_paths();
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, false),
            PathBuf::from("/home/testuser/.claude/agents")
        );
    }

    #[test]
    fn install_dir_skill_global_returns_home_claude_skills() {
        let paths = test_paths();
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, false),
            PathBuf::from("/home/testuser/.claude/skills")
        );
    }

    #[test]
    fn install_dir_agent_local_returns_relative_claude_agents() {
        let paths = test_paths();
        assert_eq!(paths.install_dir(ArtifactKind::Agent, true), PathBuf::from(".claude/agents"));
    }

    #[test]
    fn install_dir_skill_local_returns_relative_claude_skills() {
        let paths = test_paths();
        assert_eq!(paths.install_dir(ArtifactKind::Skill, true), PathBuf::from(".claude/skills"));
    }

    // --- Cursor ---

    #[test]
    fn install_dir_cursor_agent_local_returns_cursor_agents() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(paths.install_dir(ArtifactKind::Agent, true), PathBuf::from(".cursor/agents"));
    }

    #[test]
    fn install_dir_cursor_skill_local_returns_cursor_skills() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(paths.install_dir(ArtifactKind::Skill, true), PathBuf::from(".cursor/skills"));
    }

    #[test]
    fn install_dir_cursor_agent_global_returns_home_cursor_agents() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, false),
            PathBuf::from("/home/testuser/.cursor/agents")
        );
    }

    #[test]
    fn install_dir_cursor_skill_global_returns_home_cursor_skills() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, false),
            PathBuf::from("/home/testuser/.cursor/skills")
        );
    }

    #[test]
    fn lock_path_cursor_global_uses_cursor_slug() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.lock_path(false),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-cursor.json")
        );
    }

    #[test]
    fn lock_path_cursor_local_uses_cursor_slug() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(paths.lock_path(true), PathBuf::from(".context-mixer/cmx-lock-cursor.json"));
    }

    // --- Copilot ---

    #[test]
    fn install_dir_copilot_agent_local_returns_github_agents() {
        let paths = test_paths_for(Platform::Copilot);
        assert_eq!(paths.install_dir(ArtifactKind::Agent, true), PathBuf::from(".github/agents"));
    }

    #[test]
    fn install_dir_copilot_agent_global_returns_home_copilot_agents() {
        let paths = test_paths_for(Platform::Copilot);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, false),
            PathBuf::from("/home/testuser/.copilot/agents")
        );
    }

    #[test]
    fn lock_path_copilot_global_uses_copilot_slug() {
        let paths = test_paths_for(Platform::Copilot);
        assert_eq!(
            paths.lock_path(false),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-copilot.json")
        );
    }

    // --- Windsurf ---

    #[test]
    fn install_dir_windsurf_skill_global_returns_codeium_windsurf_skills() {
        let paths = test_paths_for(Platform::Windsurf);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, false),
            PathBuf::from("/home/testuser/.codeium/windsurf/skills")
        );
    }

    #[test]
    fn install_dir_windsurf_agent_local_returns_windsurf_agents() {
        let paths = test_paths_for(Platform::Windsurf);
        assert_eq!(paths.install_dir(ArtifactKind::Agent, true), PathBuf::from(".windsurf/agents"));
    }

    #[test]
    fn lock_path_windsurf_global_uses_windsurf_slug() {
        let paths = test_paths_for(Platform::Windsurf);
        assert_eq!(
            paths.lock_path(false),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-windsurf.json")
        );
    }

    // --- Gemini ---

    #[test]
    fn install_dir_gemini_agent_global_returns_home_gemini_agents() {
        let paths = test_paths_for(Platform::Gemini);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, false),
            PathBuf::from("/home/testuser/.gemini/agents")
        );
    }

    #[test]
    fn lock_path_gemini_global_uses_gemini_slug() {
        let paths = test_paths_for(Platform::Gemini);
        assert_eq!(
            paths.lock_path(false),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-gemini.json")
        );
    }
}
