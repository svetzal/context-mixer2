use std::fmt;
use std::path::PathBuf;

use crate::types::{ArtifactKind, InstallScope};

/// The target AI coding assistant platform for artifact installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Platform {
    #[default]
    Claude,
    Copilot,
    Cursor,
    Windsurf,
    Gemini,
    /// opencode — markdown agents; skills in the shared .agents directory.
    Opencode,
    /// Codex CLI — TOML agents; skills in the shared .agents directory.
    Codex,
    /// Pi — skills only; no native agent concept.
    Pi,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Claude => "claude",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Codex => "codex",
            Self::Pi => "pi",
        };
        f.write_str(name)
    }
}

impl Platform {
    /// The install directory for the given artifact kind and scope.
    ///
    /// The returned path is relative — to the project root for
    /// [`InstallScope::Local`], or to `$HOME` for [`InstallScope::Global`]. It
    /// already includes the kind-specific leaf directory (e.g. `agents`,
    /// `skills`), so callers do not append a subdirectory.
    ///
    /// For an unsupported `(platform, kind)` combination (see [`supports`]) this
    /// returns a platform-namespaced fallback that is never written to;
    /// [`supports`] gates the mutating commands before any path is used.
    ///
    /// [`supports`]: Platform::supports
    pub fn install_subpath(self, kind: ArtifactKind, scope: InstallScope) -> PathBuf {
        use ArtifactKind::{Agent, Skill};

        // Two-component relative path helper.
        let dir = |base: &str, leaf: &str| PathBuf::from(base).join(leaf);
        let local = scope.is_local();

        match (self, kind) {
            (Self::Claude, Agent) => dir(".claude", "agents"),
            (Self::Claude, Skill) => dir(".claude", "skills"),

            // Copilot reads project files from `.github` but stores user-scoped
            // artifacts under `.copilot`.
            (Self::Copilot, Agent) => dir(if local { ".github" } else { ".copilot" }, "agents"),
            (Self::Copilot, Skill) => dir(if local { ".github" } else { ".copilot" }, "skills"),

            (Self::Cursor, Agent) => dir(".cursor", "agents"),
            (Self::Cursor, Skill) => dir(".cursor", "skills"),

            // Windsurf nests user-scoped artifacts under `.codeium/windsurf`.
            (Self::Windsurf, Agent) if local => dir(".windsurf", "agents"),
            (Self::Windsurf, Skill) if local => dir(".windsurf", "skills"),
            (Self::Windsurf, Agent) => PathBuf::from(".codeium").join("windsurf").join("agents"),
            (Self::Windsurf, Skill) => PathBuf::from(".codeium").join("windsurf").join("skills"),

            (Self::Gemini, Agent) => dir(".gemini", "agents"),
            (Self::Gemini, Skill) => dir(".gemini", "skills"),

            // opencode: markdown agents in `.opencode/agent` (singular leaf,
            // which the loader reads alongside the plural form); user-scoped
            // config lives under `~/.config/opencode`.
            (Self::Opencode, Agent) if local => dir(".opencode", "agent"),
            (Self::Opencode, Agent) => PathBuf::from(".config").join("opencode").join("agent"),

            // codex: TOML agents in `.codex/agents` (both scopes).
            (Self::Codex, Agent) => dir(".codex", "agents"),

            // Pi has no native agent concept — fallback never written to.
            (Self::Pi, Agent) => dir(".pi", "agents"),

            // Skills for the `.agents`-standard tools resolve to the shared
            // cross-tool `.agents/skills` location (read by opencode, codex,
            // and pi), for both local and global scopes.
            (Self::Opencode | Self::Codex | Self::Pi, Skill) => {
                PathBuf::from(".agents").join("skills")
            }
        }
    }

    /// Whether this platform supports installing the given artifact kind.
    ///
    /// Pi has no native agent concept, so `(Pi, Agent)` is unsupported. Every
    /// other `(platform, kind)` combination is supported.
    pub fn supports(self, kind: ArtifactKind) -> bool {
        !matches!((self, kind), (Self::Pi, ArtifactKind::Agent))
    }

    /// The file extension for an installed agent on this platform.
    ///
    /// Most platforms store agents as markdown (`md`); codex uses TOML (`toml`)
    /// and the source markdown is transformed during install.
    pub fn agent_extension(self) -> &'static str {
        match self {
            Self::Codex => "toml",
            _ => "md",
        }
    }

    /// Whether installing an agent for this platform requires transforming the
    /// source markdown into codex's TOML subagent format.
    pub fn transforms_agent_to_toml(self) -> bool {
        matches!(self, Self::Codex)
    }

    /// Slug used to construct platform-specific lock file names.
    ///
    /// Claude returns an empty string (lock file stays `cmx-lock.json` for
    /// backward compatibility). All other platforms return a non-empty slug.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Codex => "codex",
            Self::Pi => "pi",
        }
    }

    /// The directory name for this platform's plugin manifest (used by cmf).
    ///
    /// Only the platforms returned by [`targets`] consume this; the
    /// `.agents`-standard tools (opencode, codex, pi) have no Claude-style
    /// plugin manifest, so their value here is unused.
    ///
    /// [`targets`]: Platform::targets
    pub fn manifest_dir(self) -> &'static str {
        match self {
            Self::Claude => ".claude-plugin",
            Self::Copilot => ".copilot-plugin",
            Self::Cursor => ".cursor-plugin",
            Self::Windsurf => ".windsurf-plugin",
            Self::Gemini => ".gemini-plugin",
            Self::Opencode => ".opencode-plugin",
            Self::Codex => ".codex-plugin",
            Self::Pi => ".pi-plugin",
        }
    }

    /// All non-Claude platforms that receive generated plugin manifests.
    ///
    /// opencode, codex, and pi are intentionally excluded: none of them define a
    /// plugin/marketplace manifest format, so generating one would produce dead
    /// files no tool reads.
    pub fn targets() -> &'static [Platform] {
        &[Self::Copilot, Self::Cursor, Self::Windsurf, Self::Gemini]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_claude() {
        assert_eq!(Platform::default(), Platform::Claude);
    }

    #[test]
    fn display_produces_lowercase_names() {
        assert_eq!(Platform::Claude.to_string(), "claude");
        assert_eq!(Platform::Copilot.to_string(), "copilot");
        assert_eq!(Platform::Cursor.to_string(), "cursor");
        assert_eq!(Platform::Windsurf.to_string(), "windsurf");
        assert_eq!(Platform::Gemini.to_string(), "gemini");
        assert_eq!(Platform::Opencode.to_string(), "opencode");
        assert_eq!(Platform::Codex.to_string(), "codex");
        assert_eq!(Platform::Pi.to_string(), "pi");
    }

    #[test]
    fn manifest_dir_values() {
        assert_eq!(Platform::Claude.manifest_dir(), ".claude-plugin");
        assert_eq!(Platform::Copilot.manifest_dir(), ".copilot-plugin");
        assert_eq!(Platform::Cursor.manifest_dir(), ".cursor-plugin");
        assert_eq!(Platform::Windsurf.manifest_dir(), ".windsurf-plugin");
        assert_eq!(Platform::Gemini.manifest_dir(), ".gemini-plugin");
    }

    #[test]
    fn targets_includes_windsurf() {
        assert!(
            Platform::targets().contains(&Platform::Windsurf),
            "expected Windsurf in targets()"
        );
    }

    #[test]
    fn targets_excludes_claude() {
        assert!(
            !Platform::targets().contains(&Platform::Claude),
            "Claude should not be in targets()"
        );
    }

    #[test]
    fn targets_excludes_agents_standard_tools() {
        for p in [Platform::Opencode, Platform::Codex, Platform::Pi] {
            assert!(
                !Platform::targets().contains(&p),
                "{p} has no manifest format and must not be a manifest target"
            );
        }
    }

    // --- install_subpath: existing platforms (behavior preserved) ---

    #[test]
    fn install_subpath_claude() {
        assert_eq!(
            Platform::Claude.install_subpath(ArtifactKind::Agent, InstallScope::Local),
            PathBuf::from(".claude").join("agents")
        );
        assert_eq!(
            Platform::Claude.install_subpath(ArtifactKind::Skill, InstallScope::Global),
            PathBuf::from(".claude").join("skills")
        );
    }

    #[test]
    fn install_subpath_copilot_differs_by_scope() {
        assert_eq!(
            Platform::Copilot.install_subpath(ArtifactKind::Agent, InstallScope::Local),
            PathBuf::from(".github").join("agents")
        );
        assert_eq!(
            Platform::Copilot.install_subpath(ArtifactKind::Agent, InstallScope::Global),
            PathBuf::from(".copilot").join("agents")
        );
    }

    #[test]
    fn install_subpath_windsurf_global_nests_under_codeium() {
        assert_eq!(
            Platform::Windsurf.install_subpath(ArtifactKind::Skill, InstallScope::Global),
            PathBuf::from(".codeium").join("windsurf").join("skills")
        );
        assert_eq!(
            Platform::Windsurf.install_subpath(ArtifactKind::Agent, InstallScope::Local),
            PathBuf::from(".windsurf").join("agents")
        );
    }

    // --- install_subpath: new platforms ---

    #[test]
    fn install_subpath_opencode_agent_uses_singular_leaf_and_xdg_global() {
        assert_eq!(
            Platform::Opencode.install_subpath(ArtifactKind::Agent, InstallScope::Local),
            PathBuf::from(".opencode").join("agent")
        );
        assert_eq!(
            Platform::Opencode.install_subpath(ArtifactKind::Agent, InstallScope::Global),
            PathBuf::from(".config").join("opencode").join("agent")
        );
    }

    #[test]
    fn install_subpath_codex_agent_uses_dot_codex_agents() {
        assert_eq!(
            Platform::Codex.install_subpath(ArtifactKind::Agent, InstallScope::Local),
            PathBuf::from(".codex").join("agents")
        );
        assert_eq!(
            Platform::Codex.install_subpath(ArtifactKind::Agent, InstallScope::Global),
            PathBuf::from(".codex").join("agents")
        );
    }

    #[test]
    fn install_subpath_new_tools_share_dot_agents_skills() {
        for p in [Platform::Opencode, Platform::Codex, Platform::Pi] {
            for scope in InstallScope::ALL {
                assert_eq!(
                    p.install_subpath(ArtifactKind::Skill, scope),
                    PathBuf::from(".agents").join("skills"),
                    "{p} skills (scope {scope:?}) should resolve to .agents/skills"
                );
            }
        }
    }

    // --- supports ---

    #[test]
    fn supports_pi_skills_but_not_agents() {
        assert!(Platform::Pi.supports(ArtifactKind::Skill));
        assert!(!Platform::Pi.supports(ArtifactKind::Agent));
    }

    #[test]
    fn supports_all_kinds_for_other_platforms() {
        for p in [
            Platform::Claude,
            Platform::Copilot,
            Platform::Cursor,
            Platform::Windsurf,
            Platform::Gemini,
            Platform::Opencode,
            Platform::Codex,
        ] {
            assert!(p.supports(ArtifactKind::Agent), "{p} should support agents");
            assert!(p.supports(ArtifactKind::Skill), "{p} should support skills");
        }
    }

    // --- agent_extension / transform ---

    #[test]
    fn agent_extension_codex_is_toml_others_md() {
        assert_eq!(Platform::Codex.agent_extension(), "toml");
        assert_eq!(Platform::Claude.agent_extension(), "md");
        assert_eq!(Platform::Opencode.agent_extension(), "md");
        assert_eq!(Platform::Pi.agent_extension(), "md");
    }

    #[test]
    fn only_codex_transforms_agents() {
        assert!(Platform::Codex.transforms_agent_to_toml());
        assert!(!Platform::Claude.transforms_agent_to_toml());
        assert!(!Platform::Opencode.transforms_agent_to_toml());
    }

    // --- slug ---

    #[test]
    fn slug_values() {
        assert_eq!(Platform::Claude.slug(), "");
        assert_eq!(Platform::Copilot.slug(), "copilot");
        assert_eq!(Platform::Cursor.slug(), "cursor");
        assert_eq!(Platform::Windsurf.slug(), "windsurf");
        assert_eq!(Platform::Gemini.slug(), "gemini");
        assert_eq!(Platform::Opencode.slug(), "opencode");
        assert_eq!(Platform::Codex.slug(), "codex");
        assert_eq!(Platform::Pi.slug(), "pi");
    }

    #[test]
    fn value_enum_parses_all_variants() {
        use clap::ValueEnum;
        assert_eq!(Platform::from_str("claude", true).unwrap(), Platform::Claude);
        assert_eq!(Platform::from_str("copilot", true).unwrap(), Platform::Copilot);
        assert_eq!(Platform::from_str("cursor", true).unwrap(), Platform::Cursor);
        assert_eq!(Platform::from_str("windsurf", true).unwrap(), Platform::Windsurf);
        assert_eq!(Platform::from_str("gemini", true).unwrap(), Platform::Gemini);
        assert_eq!(Platform::from_str("opencode", true).unwrap(), Platform::Opencode);
        assert_eq!(Platform::from_str("codex", true).unwrap(), Platform::Codex);
        assert_eq!(Platform::from_str("pi", true).unwrap(), Platform::Pi);
    }
}
