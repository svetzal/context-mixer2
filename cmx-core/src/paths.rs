use std::path::PathBuf;

use crate::error::{CmxError, Result};
use crate::gateway::Filesystem;
use crate::platform::Platform;
use crate::types::{ArtifactKind, InstallScope};

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
        let home = dirs::home_dir().ok_or(CmxError::HomeDirUnavailable)?;
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

    /// Return a view of these paths bound to a different platform.
    ///
    /// `home_dir` and `config_dir` are platform-independent — only the active
    /// platform changes. `cmx doctor` uses this to survey every platform's
    /// install directories and lock files from a single base, reusing all the
    /// platform-aware path resolution without rebuilding it per platform.
    #[must_use]
    pub fn with_platform(&self, platform: Platform) -> ConfigPaths {
        ConfigPaths {
            config_dir: self.config_dir.clone(),
            home_dir: self.home_dir.clone(),
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

    /// Path to `sets.json` for the given scope.
    ///
    /// Sets are platform-independent — a single file per scope, unlike
    /// [`lock_path`](Self::lock_path) which carries a per-platform slug.
    pub fn sets_path(&self, scope: InstallScope) -> PathBuf {
        if scope.is_local() {
            PathBuf::from(".context-mixer").join("sets.json")
        } else {
            self.config_dir.join("sets.json")
        }
    }

    /// Path to `config.json` (LLM gateway settings).
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    /// Default location of the canonical artifact home — under cmx's existing
    /// config root, alongside `sources.json` and the lockfiles.
    ///
    /// This is the *default*; the `home` field in `config.json` can override it.
    /// Use [`crate::config::resolve_artifact_home`] to get the effective home,
    /// which consults the config first. Note this is unrelated to
    /// [`home_dir`](Self::home_dir), which is the OS home (`$HOME`).
    pub fn default_artifact_home(&self) -> PathBuf {
        self.config_dir.join("home")
    }

    /// Path to the lock file for the given scope.
    ///
    /// Claude uses `cmx-lock.json` for backward compatibility. All other
    /// platforms use `cmx-lock-<slug>.json`.
    pub fn lock_path(&self, scope: InstallScope) -> PathBuf {
        let file_name = if self.platform.slug().is_empty() {
            "cmx-lock.json".to_string()
        } else {
            format!("cmx-lock-{}.json", self.platform.slug())
        };

        if scope.is_local() {
            PathBuf::from(".context-mixer").join(&file_name)
        } else {
            self.config_dir.join(&file_name)
        }
    }

    /// Directory where artifacts of the given kind and scope are installed.
    ///
    /// Resolution is delegated to [`Platform::install_subpath`], which encodes
    /// each platform's layout (including per-kind divergence such as codex/pi
    /// skills living under the shared `.agents/skills`). Local installs are
    /// relative to the project root; global installs are anchored at `$HOME`.
    ///
    /// Returns `None` for unsupported `(platform, kind)` combinations. Callers
    /// should gate on [`ensure_supports`](Self::ensure_supports) or
    /// [`Platform::supports`] before calling this.
    pub fn install_dir(&self, kind: ArtifactKind, scope: InstallScope) -> Option<PathBuf> {
        let subpath = self.platform.install_subpath(kind, scope)?;
        Some(if scope.is_local() {
            subpath
        } else {
            self.home_dir.join(subpath)
        })
    }

    /// Full path to where an artifact of `kind` named `name` is (or would be)
    /// installed under `scope`, accounting for the platform's agent file format.
    ///
    /// Agents use the platform's [`agent_extension`](Platform::agent_extension)
    /// (e.g. `.md`, or `.toml` for codex); skills resolve to a directory named
    /// after the artifact.
    ///
    /// Returns `None` for unsupported `(platform, kind)` combinations.
    pub fn installed_artifact_path(
        &self,
        kind: ArtifactKind,
        name: &str,
        scope: InstallScope,
    ) -> Option<PathBuf> {
        let dir = self.install_dir(kind, scope)?;
        Some(kind.installed_path(name, &dir, self.platform.agent_extension()))
    }

    /// Returns `true` if an artifact of `kind` named `name` exists on disk under `scope`.
    ///
    /// Returns `false` for unsupported `(platform, kind)` combinations.
    pub fn is_installed(
        &self,
        kind: ArtifactKind,
        name: &str,
        scope: InstallScope,
        fs: &dyn Filesystem,
    ) -> bool {
        self.installed_artifact_path(kind, name, scope)
            .is_some_and(|path| fs.exists(&path))
    }

    /// Verify the active platform supports the given artifact kind, returning a
    /// user-facing error otherwise (e.g. pi has no agent concept).
    pub fn ensure_supports(&self, kind: ArtifactKind) -> Result<()> {
        if self.platform.supports(kind) {
            Ok(())
        } else {
            Err(unsupported_artifact_error(self.platform, kind))
        }
    }

    /// Like [`install_dir`](Self::install_dir), but returns `Err` for unsupported
    /// `(platform, kind)` combinations instead of `None`.
    pub fn require_install_dir(&self, kind: ArtifactKind, scope: InstallScope) -> Result<PathBuf> {
        self.install_dir(kind, scope)
            .ok_or_else(|| unsupported_artifact_error(self.platform, kind))
    }

    /// Like [`installed_artifact_path`](Self::installed_artifact_path), but returns
    /// `Err` for unsupported `(platform, kind)` combinations instead of `None`.
    pub fn require_installed_artifact_path(
        &self,
        kind: ArtifactKind,
        name: &str,
        scope: InstallScope,
    ) -> Result<PathBuf> {
        self.installed_artifact_path(kind, name, scope)
            .ok_or_else(|| unsupported_artifact_error(self.platform, kind))
    }
}

fn unsupported_artifact_error(platform: Platform, kind: ArtifactKind) -> CmxError {
    CmxError::UnsupportedArtifact { platform, kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::types::InstallScope;

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

    // --- is_installed ---

    #[test]
    fn is_installed_returns_false_for_absent_artifact() {
        let paths = test_paths();
        let fs = FakeFilesystem::new();
        assert!(!paths.is_installed(ArtifactKind::Agent, "my-agent", InstallScope::Global, &fs));
    }

    #[test]
    fn is_installed_returns_true_when_file_present() {
        let paths = test_paths();
        let fs = FakeFilesystem::new();
        let path = paths
            .installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Global)
            .unwrap();
        fs.add_file(path, "# agent");
        assert!(paths.is_installed(ArtifactKind::Agent, "my-agent", InstallScope::Global, &fs));
    }

    // --- with_platform ---

    #[test]
    fn with_platform_rebinds_platform_keeping_dirs() {
        let base = test_paths(); // Claude
        let codex = base.with_platform(Platform::Codex);
        assert_eq!(codex.platform, Platform::Codex);
        // home_dir and config_dir are carried over unchanged.
        assert_eq!(codex.config_dir, base.config_dir);
        assert_eq!(codex.home_dir, base.home_dir);
        // Path resolution now reflects the new platform.
        assert_eq!(
            codex.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-codex.json")
        );
        assert_eq!(
            codex.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.codex/agents")
        );
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
    fn sets_path_global_returns_config_dir_sets_json() {
        let paths = test_paths();
        assert_eq!(
            paths.sets_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/sets.json")
        );
    }

    #[test]
    fn sets_path_local_returns_relative_path() {
        let paths = test_paths();
        assert_eq!(paths.sets_path(InstallScope::Local), PathBuf::from(".context-mixer/sets.json"));
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
            paths.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock.json")
        );
    }

    #[test]
    fn lock_path_local_returns_relative_path() {
        let paths = test_paths();
        assert_eq!(
            paths.lock_path(InstallScope::Local),
            PathBuf::from(".context-mixer/cmx-lock.json")
        );
    }

    #[test]
    fn install_dir_agent_global_returns_home_claude_agents() {
        let paths = test_paths();
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.claude/agents")
        );
    }

    #[test]
    fn install_dir_skill_global_returns_home_claude_skills() {
        let paths = test_paths();
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.claude/skills")
        );
    }

    #[test]
    fn install_dir_agent_local_returns_relative_claude_agents() {
        let paths = test_paths();
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap(),
            PathBuf::from(".claude/agents")
        );
    }

    #[test]
    fn install_dir_skill_local_returns_relative_claude_skills() {
        let paths = test_paths();
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Local).unwrap(),
            PathBuf::from(".claude/skills")
        );
    }

    // --- Cursor ---

    #[test]
    fn install_dir_cursor_agent_local_returns_cursor_agents() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap(),
            PathBuf::from(".cursor/agents")
        );
    }

    #[test]
    fn install_dir_cursor_skill_local_returns_cursor_skills() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Local).unwrap(),
            PathBuf::from(".cursor/skills")
        );
    }

    #[test]
    fn install_dir_cursor_agent_global_returns_home_cursor_agents() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.cursor/agents")
        );
    }

    #[test]
    fn install_dir_cursor_skill_global_returns_home_cursor_skills() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.cursor/skills")
        );
    }

    #[test]
    fn lock_path_cursor_global_uses_cursor_slug() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-cursor.json")
        );
    }

    #[test]
    fn lock_path_cursor_local_uses_cursor_slug() {
        let paths = test_paths_for(Platform::Cursor);
        assert_eq!(
            paths.lock_path(InstallScope::Local),
            PathBuf::from(".context-mixer/cmx-lock-cursor.json")
        );
    }

    // --- Copilot ---

    #[test]
    fn install_dir_copilot_agent_local_returns_github_agents() {
        let paths = test_paths_for(Platform::Copilot);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap(),
            PathBuf::from(".github/agents")
        );
    }

    #[test]
    fn install_dir_copilot_agent_global_returns_home_copilot_agents() {
        let paths = test_paths_for(Platform::Copilot);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.copilot/agents")
        );
    }

    #[test]
    fn lock_path_copilot_global_uses_copilot_slug() {
        let paths = test_paths_for(Platform::Copilot);
        assert_eq!(
            paths.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-copilot.json")
        );
    }

    // --- Windsurf ---

    #[test]
    fn install_dir_windsurf_skill_global_returns_codeium_windsurf_skills() {
        let paths = test_paths_for(Platform::Windsurf);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.codeium/windsurf/skills")
        );
    }

    #[test]
    fn install_dir_windsurf_agent_local_returns_windsurf_agents() {
        let paths = test_paths_for(Platform::Windsurf);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap(),
            PathBuf::from(".windsurf/agents")
        );
    }

    #[test]
    fn lock_path_windsurf_global_uses_windsurf_slug() {
        let paths = test_paths_for(Platform::Windsurf);
        assert_eq!(
            paths.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-windsurf.json")
        );
    }

    // --- Gemini ---

    #[test]
    fn install_dir_gemini_agent_global_returns_home_gemini_agents() {
        let paths = test_paths_for(Platform::Gemini);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.gemini/agents")
        );
    }

    #[test]
    fn lock_path_gemini_global_uses_gemini_slug() {
        let paths = test_paths_for(Platform::Gemini);
        assert_eq!(
            paths.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-gemini.json")
        );
    }

    // --- opencode ---

    #[test]
    fn install_dir_opencode_agent_local_uses_singular_leaf() {
        let paths = test_paths_for(Platform::Opencode);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap(),
            PathBuf::from(".opencode/agent")
        );
    }

    #[test]
    fn install_dir_opencode_agent_global_uses_xdg_config() {
        let paths = test_paths_for(Platform::Opencode);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.config/opencode/agent")
        );
    }

    #[test]
    fn install_dir_opencode_skill_uses_shared_dot_agents() {
        let paths = test_paths_for(Platform::Opencode);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Local).unwrap(),
            PathBuf::from(".agents/skills")
        );
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.agents/skills")
        );
    }

    #[test]
    fn lock_path_opencode_uses_opencode_slug() {
        let paths = test_paths_for(Platform::Opencode);
        assert_eq!(
            paths.lock_path(InstallScope::Global),
            PathBuf::from("/home/testuser/.config/context-mixer/cmx-lock-opencode.json")
        );
    }

    // --- codex ---

    #[test]
    fn install_dir_codex_agent_uses_dot_codex_agents() {
        let paths = test_paths_for(Platform::Codex);
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap(),
            PathBuf::from(".codex/agents")
        );
        assert_eq!(
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.codex/agents")
        );
    }

    #[test]
    fn install_dir_codex_skill_uses_shared_dot_agents() {
        let paths = test_paths_for(Platform::Codex);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap(),
            PathBuf::from("/home/testuser/.agents/skills")
        );
    }

    #[test]
    fn installed_artifact_path_codex_agent_is_toml() {
        let paths = test_paths_for(Platform::Codex);
        assert_eq!(
            paths
                .installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Global)
                .unwrap(),
            PathBuf::from("/home/testuser/.codex/agents/my-agent.toml")
        );
    }

    #[test]
    fn installed_artifact_path_default_agent_is_md() {
        let paths = test_paths();
        assert_eq!(
            paths
                .installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Local)
                .unwrap(),
            PathBuf::from(".claude/agents/my-agent.md")
        );
    }

    #[test]
    fn installed_artifact_path_skill_is_directory() {
        let paths = test_paths_for(Platform::Codex);
        assert_eq!(
            paths
                .installed_artifact_path(ArtifactKind::Skill, "my-skill", InstallScope::Local)
                .unwrap(),
            PathBuf::from(".agents/skills/my-skill")
        );
    }

    // --- pi ---

    #[test]
    fn install_dir_pi_skill_uses_shared_dot_agents() {
        let paths = test_paths_for(Platform::Pi);
        assert_eq!(
            paths.install_dir(ArtifactKind::Skill, InstallScope::Local).unwrap(),
            PathBuf::from(".agents/skills")
        );
    }

    // --- require_install_dir ---

    #[test]
    fn require_install_dir_returns_ok_for_supported_combo() {
        let paths = test_paths(); // Claude platform
        let result = paths.require_install_dir(ArtifactKind::Skill, InstallScope::Global);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from("/home/testuser/.claude/skills"));
    }

    #[test]
    fn require_install_dir_returns_err_for_unsupported_combo() {
        let paths = test_paths_for(Platform::Pi);
        let err = paths
            .require_install_dir(ArtifactKind::Agent, InstallScope::Global)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pi"), "error should name the platform: {msg}");
        assert!(msg.contains("agent"), "error should name the kind: {msg}");
    }

    // --- require_installed_artifact_path ---

    #[test]
    fn require_installed_artifact_path_returns_ok_for_supported_combo() {
        let paths = test_paths();
        let result = paths.require_installed_artifact_path(
            ArtifactKind::Agent,
            "my-agent",
            InstallScope::Global,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from("/home/testuser/.claude/agents/my-agent.md"));
    }

    #[test]
    fn require_installed_artifact_path_returns_err_for_unsupported_combo() {
        let paths = test_paths_for(Platform::Pi);
        let err = paths
            .require_installed_artifact_path(ArtifactKind::Agent, "x", InstallScope::Global)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("pi"), "error should name the platform: {msg}");
        assert!(msg.contains("agent"), "error should name the kind: {msg}");
    }

    #[test]
    fn ensure_supports_pi_rejects_agents() {
        let paths = test_paths_for(Platform::Pi);
        let err = paths.ensure_supports(ArtifactKind::Agent).unwrap_err().to_string();
        assert!(err.contains("pi"), "error should name the platform: {err}");
        assert!(err.contains("agent"), "error should name the kind: {err}");
    }

    #[test]
    fn ensure_supports_pi_allows_skills() {
        let paths = test_paths_for(Platform::Pi);
        assert!(paths.ensure_supports(ArtifactKind::Skill).is_ok());
    }

    #[test]
    fn ensure_supports_codex_allows_both() {
        let paths = test_paths_for(Platform::Codex);
        assert!(paths.ensure_supports(ArtifactKind::Agent).is_ok());
        assert!(paths.ensure_supports(ArtifactKind::Skill).is_ok());
    }
}
