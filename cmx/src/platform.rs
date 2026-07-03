use std::fmt;
use std::path::PathBuf;

use crate::types::{ArtifactKind, InstallScope};

// --- private spec types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentFormat {
    Markdown,
    Toml,
}

struct PlatformSpec {
    name: &'static str,
    slug: &'static str,
    /// `Some` only for platforms that define a plugin-manifest format (used by
    /// cmf). `None` for `.agents`-standard tools, which have no such format.
    manifest_dir: Option<&'static str>,
    /// `Some` for platforms that support file-droppable agents; the variant
    /// selects the installed format. `None` for skills-only tools.
    agent_format: Option<AgentFormat>,
}

// --- enum ---

/// The target AI coding assistant platform for artifact installation.
///
/// Serializes to its lowercase name (`"claude"`, `"codex"`, …) — the same token
/// the `--platform` flag accepts — so the `platforms` list in `config.json`
/// stays human-readable and round-trips with the CLI.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    clap::ValueEnum,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
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
    /// Crush — skills only; reads the shared .agents directory.
    Crush,
    /// Amp — skills only; reads the shared .agents directory.
    Amp,
    /// Zed — skills only; agents are settings-embedded profiles cmx does not manage.
    Zed,
    /// openhands — skills only; agents are trigger-activated skills.
    Openhands,
    /// Hermes — skills only; global-centric (~/.hermes/skills); no agent files.
    Hermes,
    /// Devin — skills only; discovers `SKILL.md` in connected repos and reads
    /// the shared .agents directory (its recommended location); no agent files.
    Devin,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.spec().name)
    }
}

impl Platform {
    /// Single authoritative per-variant data table.
    ///
    /// The exhaustive `match` forces any new variant to be listed here — the
    /// compiler rejects a build that adds a 14th variant without filling in its
    /// spec, eliminating the need for parallel matches elsewhere.
    fn spec(self) -> PlatformSpec {
        match self {
            // Claude is the canonical manifest *source* (`.claude-plugin/`), not a
            // target — cmf reads from it but never writes back to it.
            Self::Claude => PlatformSpec {
                name: "claude",
                slug: "",
                manifest_dir: None,
                agent_format: Some(AgentFormat::Markdown),
            },
            Self::Copilot => PlatformSpec {
                name: "copilot",
                slug: "copilot",
                manifest_dir: Some(".copilot-plugin"),
                agent_format: Some(AgentFormat::Markdown),
            },
            Self::Cursor => PlatformSpec {
                name: "cursor",
                slug: "cursor",
                manifest_dir: Some(".cursor-plugin"),
                agent_format: Some(AgentFormat::Markdown),
            },
            Self::Windsurf => PlatformSpec {
                name: "windsurf",
                slug: "windsurf",
                manifest_dir: Some(".windsurf-plugin"),
                agent_format: Some(AgentFormat::Markdown),
            },
            Self::Gemini => PlatformSpec {
                name: "gemini",
                slug: "gemini",
                manifest_dir: Some(".gemini-plugin"),
                agent_format: Some(AgentFormat::Markdown),
            },
            Self::Opencode => PlatformSpec {
                name: "opencode",
                slug: "opencode",
                manifest_dir: None,
                agent_format: Some(AgentFormat::Markdown),
            },
            Self::Codex => PlatformSpec {
                name: "codex",
                slug: "codex",
                manifest_dir: None,
                agent_format: Some(AgentFormat::Toml),
            },
            Self::Pi => PlatformSpec {
                name: "pi",
                slug: "pi",
                manifest_dir: None,
                agent_format: None,
            },
            Self::Crush => PlatformSpec {
                name: "crush",
                slug: "crush",
                manifest_dir: None,
                agent_format: None,
            },
            Self::Amp => PlatformSpec {
                name: "amp",
                slug: "amp",
                manifest_dir: None,
                agent_format: None,
            },
            Self::Zed => PlatformSpec {
                name: "zed",
                slug: "zed",
                manifest_dir: None,
                agent_format: None,
            },
            Self::Openhands => PlatformSpec {
                name: "openhands",
                slug: "openhands",
                manifest_dir: None,
                agent_format: None,
            },
            Self::Hermes => PlatformSpec {
                name: "hermes",
                slug: "hermes",
                manifest_dir: None,
                agent_format: None,
            },
            Self::Devin => PlatformSpec {
                name: "devin",
                slug: "devin",
                manifest_dir: None,
                agent_format: None,
            },
        }
    }

    /// The install directory for the given artifact kind and scope.
    ///
    /// The returned path is relative — to the project root for
    /// [`InstallScope::Local`], or to `$HOME` for [`InstallScope::Global`]. It
    /// already includes the kind-specific leaf directory (e.g. `agents`,
    /// `skills`), so callers do not append a subdirectory.
    ///
    /// Returns `None` for unsupported `(platform, kind)` combinations (see
    /// [`supports`]). Callers should gate on `supports` or `ensure_supports`
    /// before calling this, so `None` indicates a programming error at the call
    /// site.
    ///
    /// [`supports`]: Platform::supports
    pub fn install_subpath(self, kind: ArtifactKind, scope: InstallScope) -> Option<PathBuf> {
        use ArtifactKind::{Agent, Skill};

        let dir = |base: &str, leaf: &str| PathBuf::from(base).join(leaf);
        let local = scope.is_local();

        match (self, kind) {
            (Self::Claude, Agent) => Some(dir(".claude", "agents")),
            (Self::Claude, Skill) => Some(dir(".claude", "skills")),

            // Copilot reads project files from `.github` but stores user-scoped
            // artifacts under `.copilot`.
            (Self::Copilot, Agent) => {
                Some(dir(if local { ".github" } else { ".copilot" }, "agents"))
            }
            (Self::Copilot, Skill) => {
                Some(dir(if local { ".github" } else { ".copilot" }, "skills"))
            }

            (Self::Cursor, Agent) => Some(dir(".cursor", "agents")),
            (Self::Cursor, Skill) => Some(dir(".cursor", "skills")),

            // Windsurf nests user-scoped artifacts under `.codeium/windsurf`.
            (Self::Windsurf, Agent) if local => Some(dir(".windsurf", "agents")),
            (Self::Windsurf, Skill) if local => Some(dir(".windsurf", "skills")),
            (Self::Windsurf, Agent) => {
                Some(PathBuf::from(".codeium").join("windsurf").join("agents"))
            }
            (Self::Windsurf, Skill) => {
                Some(PathBuf::from(".codeium").join("windsurf").join("skills"))
            }

            (Self::Gemini, Agent) => Some(dir(".gemini", "agents")),
            (Self::Gemini, Skill) => Some(dir(".gemini", "skills")),

            // opencode: markdown agents in `.opencode/agent` (singular leaf);
            // user-scoped config lives under `~/.config/opencode`.
            (Self::Opencode, Agent) if local => Some(dir(".opencode", "agent")),
            (Self::Opencode, Agent) => {
                Some(PathBuf::from(".config").join("opencode").join("agent"))
            }

            // codex: TOML agents in `.codex/agents` (both scopes).
            (Self::Codex, Agent) => Some(dir(".codex", "agents")),

            // Skills-only tools have no droppable agent concept.
            (
                Self::Pi
                | Self::Crush
                | Self::Amp
                | Self::Zed
                | Self::Openhands
                | Self::Hermes
                | Self::Devin,
                Agent,
            ) => None,

            // Amp and Hermes diverge only at *global* scope.
            (Self::Amp, Skill) if !local => {
                Some(PathBuf::from(".config").join("agents").join("skills"))
            }
            (Self::Hermes, Skill) if !local => Some(dir(".hermes", "skills")),

            // All `.agents`-standard tools (plus Amp/Hermes at project scope)
            // resolve skills to the shared cross-tool `.agents/skills` location.
            (
                Self::Opencode
                | Self::Codex
                | Self::Pi
                | Self::Crush
                | Self::Zed
                | Self::Openhands
                | Self::Devin
                | Self::Amp
                | Self::Hermes,
                Skill,
            ) => Some(PathBuf::from(".agents").join("skills")),
        }
    }

    /// Whether this platform supports installing the given artifact kind.
    ///
    /// Skills-only tools (Pi, Crush, Amp, Zed, `OpenHands`, Hermes) have no
    /// file-droppable agent concept; `(platform, Agent)` is unsupported for
    /// them. Derived from [`spec`](Self::spec) — a `None` `agent_format` means
    /// skills-only.
    pub fn supports(self, kind: ArtifactKind) -> bool {
        kind == ArtifactKind::Skill || self.spec().agent_format.is_some()
    }

    /// The file extension for an installed agent on this platform.
    ///
    /// Most platforms store agents as markdown (`md`); codex uses TOML (`toml`)
    /// and the source markdown is transformed during install.
    pub fn agent_extension(self) -> &'static str {
        match self.spec().agent_format {
            Some(AgentFormat::Toml) => "toml",
            _ => "md",
        }
    }

    /// Whether installing an agent for this platform requires transforming the
    /// source markdown into codex's TOML subagent format.
    pub fn transforms_agent_to_toml(self) -> bool {
        matches!(self.spec().agent_format, Some(AgentFormat::Toml))
    }

    /// Slug used to construct platform-specific lock file names.
    ///
    /// Claude returns an empty string (lock file stays `cmx-lock.json` for
    /// backward compatibility). All other platforms return a non-empty slug.
    pub fn slug(self) -> &'static str {
        self.spec().slug
    }

    /// The directory name for this platform's plugin manifest (used by cmf).
    ///
    /// Returns `Some` only for platforms that receive generated manifest copies
    /// (Copilot, Cursor, Windsurf, Gemini). Returns `None` for Claude (which is
    /// the canonical manifest *source*, not a target) and for all
    /// `.agents`-standard tools (opencode, codex, pi, Crush, Amp, Zed,
    /// `OpenHands`, Hermes), which have no plugin-manifest format.
    ///
    /// Use [`targets`](Platform::targets) to iterate only the `Some` platforms.
    pub fn manifest_dir(self) -> Option<&'static str> {
        self.spec().manifest_dir
    }

    /// Every platform variant, for exhaustive cross-platform operations such as
    /// the system survey (`cmx doctor`).
    ///
    /// Keep this in sync with the enum; the `all_contains_every_variant` test
    /// guards against a variant being added without being listed here.
    pub const ALL: [Platform; 14] = [
        Self::Claude,
        Self::Copilot,
        Self::Cursor,
        Self::Windsurf,
        Self::Gemini,
        Self::Opencode,
        Self::Codex,
        Self::Pi,
        Self::Crush,
        Self::Amp,
        Self::Zed,
        Self::Openhands,
        Self::Hermes,
        Self::Devin,
    ];

    /// All platforms that receive generated plugin manifests.
    ///
    /// Derived from `ALL` by filtering for platforms where
    /// [`manifest_dir`](Self::manifest_dir) is `Some`. The `.agents`-standard
    /// tools (opencode, codex, pi, Crush, Amp, Zed, `OpenHands`, Hermes) are
    /// excluded: none of them define a plugin/marketplace manifest format.
    pub fn targets() -> Vec<Platform> {
        Self::ALL.iter().filter(|p| p.manifest_dir().is_some()).copied().collect()
    }

    /// All manifest-target platforms paired with their manifest directory name.
    ///
    /// Each tuple `(platform, dir)` is guaranteed to have a non-empty `dir`
    /// string — callers never need to handle an `Option`. Use this instead of
    /// `targets()` when the manifest directory name is needed alongside the
    /// platform, to avoid unwrapping `manifest_dir()` at each call site.
    pub fn manifest_targets() -> Vec<(Platform, &'static str)> {
        Self::ALL.iter().filter_map(|p| p.manifest_dir().map(|dir| (*p, dir))).collect()
    }
}

/// A human-readable, comma-separated label for a set of platforms
/// (e.g. `"claude, codex"`).
///
/// The single home for what was an identical `platforms_label`/`join_platforms`
/// helper copy-pasted across the display modules and the sync core.
pub fn platforms_label(platforms: &[Platform]) -> String {
    platforms.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
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
        assert_eq!(Platform::Crush.to_string(), "crush");
        assert_eq!(Platform::Amp.to_string(), "amp");
        assert_eq!(Platform::Zed.to_string(), "zed");
        assert_eq!(Platform::Openhands.to_string(), "openhands");
        assert_eq!(Platform::Hermes.to_string(), "hermes");
        assert_eq!(Platform::Devin.to_string(), "devin");
    }

    #[test]
    fn manifest_dir_values() {
        // Manifest targets receive generated copies.
        assert_eq!(Platform::Copilot.manifest_dir(), Some(".copilot-plugin"));
        assert_eq!(Platform::Cursor.manifest_dir(), Some(".cursor-plugin"));
        assert_eq!(Platform::Windsurf.manifest_dir(), Some(".windsurf-plugin"));
        assert_eq!(Platform::Gemini.manifest_dir(), Some(".gemini-plugin"));
        // Claude is the manifest *source*, not a target.
        assert_eq!(Platform::Claude.manifest_dir(), None);
    }

    #[test]
    fn manifest_dir_none_for_non_targets() {
        for p in [
            Platform::Claude,
            Platform::Opencode,
            Platform::Codex,
            Platform::Pi,
            Platform::Crush,
            Platform::Amp,
            Platform::Zed,
            Platform::Openhands,
            Platform::Hermes,
            Platform::Devin,
        ] {
            assert!(
                p.manifest_dir().is_none(),
                "{p} is not a manifest target; manifest_dir() must be None"
            );
        }
    }

    #[test]
    fn targets_is_derived_from_all_with_manifest_dir() {
        let expected: Vec<Platform> =
            Platform::ALL.iter().filter(|p| p.manifest_dir().is_some()).copied().collect();
        assert_eq!(
            Platform::targets(),
            expected,
            "targets() must equal ALL filtered by manifest_dir"
        );
    }

    #[test]
    fn manifest_targets_pairs_each_target_with_its_dir() {
        let targets = Platform::targets();
        let manifest_targets = Platform::manifest_targets();
        assert_eq!(
            targets.len(),
            manifest_targets.len(),
            "manifest_targets() must yield the same platforms as targets()"
        );
        for (p, dir) in &manifest_targets {
            assert_eq!(
                p.manifest_dir().unwrap(),
                *dir,
                "{p}: manifest_targets dir must equal manifest_dir().unwrap()"
            );
            assert!(!dir.is_empty(), "{p}: manifest_targets dir must be non-empty");
        }
        let platforms: Vec<Platform> = manifest_targets.iter().map(|(p, _)| *p).collect();
        assert_eq!(platforms, targets, "manifest_targets() platforms must match targets() order");
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
        for p in [
            Platform::Opencode,
            Platform::Codex,
            Platform::Pi,
            Platform::Crush,
            Platform::Amp,
            Platform::Zed,
            Platform::Openhands,
            Platform::Hermes,
            Platform::Devin,
        ] {
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
            Platform::Claude
                .install_subpath(ArtifactKind::Agent, InstallScope::Local)
                .unwrap(),
            PathBuf::from(".claude").join("agents")
        );
        assert_eq!(
            Platform::Claude
                .install_subpath(ArtifactKind::Skill, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".claude").join("skills")
        );
    }

    #[test]
    fn install_subpath_copilot_differs_by_scope() {
        assert_eq!(
            Platform::Copilot
                .install_subpath(ArtifactKind::Agent, InstallScope::Local)
                .unwrap(),
            PathBuf::from(".github").join("agents")
        );
        assert_eq!(
            Platform::Copilot
                .install_subpath(ArtifactKind::Agent, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".copilot").join("agents")
        );
    }

    #[test]
    fn install_subpath_windsurf_global_nests_under_codeium() {
        assert_eq!(
            Platform::Windsurf
                .install_subpath(ArtifactKind::Skill, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".codeium").join("windsurf").join("skills")
        );
        assert_eq!(
            Platform::Windsurf
                .install_subpath(ArtifactKind::Agent, InstallScope::Local)
                .unwrap(),
            PathBuf::from(".windsurf").join("agents")
        );
    }

    // --- install_subpath: new platforms ---

    #[test]
    fn install_subpath_opencode_agent_uses_singular_leaf_and_xdg_global() {
        assert_eq!(
            Platform::Opencode
                .install_subpath(ArtifactKind::Agent, InstallScope::Local)
                .unwrap(),
            PathBuf::from(".opencode").join("agent")
        );
        assert_eq!(
            Platform::Opencode
                .install_subpath(ArtifactKind::Agent, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".config").join("opencode").join("agent")
        );
    }

    #[test]
    fn install_subpath_codex_agent_uses_dot_codex_agents() {
        assert_eq!(
            Platform::Codex
                .install_subpath(ArtifactKind::Agent, InstallScope::Local)
                .unwrap(),
            PathBuf::from(".codex").join("agents")
        );
        assert_eq!(
            Platform::Codex
                .install_subpath(ArtifactKind::Agent, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".codex").join("agents")
        );
    }

    #[test]
    fn install_subpath_new_tools_share_dot_agents_skills() {
        for p in [
            Platform::Opencode,
            Platform::Codex,
            Platform::Pi,
            Platform::Crush,
            Platform::Zed,
            Platform::Openhands,
            Platform::Devin,
        ] {
            for scope in InstallScope::ALL {
                assert_eq!(
                    p.install_subpath(ArtifactKind::Skill, scope).unwrap(),
                    PathBuf::from(".agents").join("skills"),
                    "{p} skills (scope {scope:?}) should resolve to .agents/skills"
                );
            }
        }
    }

    #[test]
    fn install_subpath_amp_skill_diverges_at_global_scope() {
        assert_eq!(
            Platform::Amp.install_subpath(ArtifactKind::Skill, InstallScope::Local).unwrap(),
            PathBuf::from(".agents").join("skills")
        );
        assert_eq!(
            Platform::Amp
                .install_subpath(ArtifactKind::Skill, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".config").join("agents").join("skills")
        );
    }

    #[test]
    fn install_subpath_hermes_skill_diverges_at_global_scope() {
        assert_eq!(
            Platform::Hermes
                .install_subpath(ArtifactKind::Skill, InstallScope::Local)
                .unwrap(),
            PathBuf::from(".agents").join("skills")
        );
        assert_eq!(
            Platform::Hermes
                .install_subpath(ArtifactKind::Skill, InstallScope::Global)
                .unwrap(),
            PathBuf::from(".hermes").join("skills")
        );
    }

    #[test]
    fn install_subpath_skills_only_tools_return_none_for_agent() {
        for p in [
            Platform::Pi,
            Platform::Crush,
            Platform::Amp,
            Platform::Zed,
            Platform::Openhands,
            Platform::Hermes,
            Platform::Devin,
        ] {
            for scope in InstallScope::ALL {
                assert!(
                    p.install_subpath(ArtifactKind::Agent, scope).is_none(),
                    "{p} has no agent concept; install_subpath(Agent, {scope:?}) must be None"
                );
            }
        }
    }

    #[test]
    fn supports_and_install_subpath_agent_agree_for_all_variants() {
        // supports(Agent) == install_subpath(Agent, _).is_some() must hold for
        // every platform at every scope — the two predicates must never diverge.
        for p in Platform::ALL {
            for scope in InstallScope::ALL {
                let has_path = p.install_subpath(ArtifactKind::Agent, scope).is_some();
                let supported = p.supports(ArtifactKind::Agent);
                assert_eq!(
                    has_path, supported,
                    "{p}: supports(Agent)={supported} but install_subpath(Agent, {scope:?}).is_some()={has_path}"
                );
            }
        }
    }

    // --- supports ---

    #[test]
    fn supports_skills_only_tools_reject_agents() {
        for p in [
            Platform::Pi,
            Platform::Crush,
            Platform::Amp,
            Platform::Zed,
            Platform::Openhands,
            Platform::Hermes,
            Platform::Devin,
        ] {
            assert!(p.supports(ArtifactKind::Skill), "{p} should support skills");
            assert!(!p.supports(ArtifactKind::Agent), "{p} should not support agents");
        }
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
    fn serializes_to_lowercase_cli_name() {
        // The config `platforms` list must round-trip with the tokens the
        // `--platform` flag and `cmx config platforms add` accept.
        assert_eq!(serde_json::to_string(&Platform::Codex).unwrap(), "\"codex\"");
        assert_eq!(serde_json::to_string(&Platform::Claude).unwrap(), "\"claude\"");
        assert_eq!(serde_json::from_str::<Platform>("\"openhands\"").unwrap(), Platform::Openhands);
    }

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
        assert_eq!(Platform::Crush.slug(), "crush");
        assert_eq!(Platform::Amp.slug(), "amp");
        assert_eq!(Platform::Zed.slug(), "zed");
        assert_eq!(Platform::Openhands.slug(), "openhands");
        assert_eq!(Platform::Hermes.slug(), "hermes");
        assert_eq!(Platform::Devin.slug(), "devin");
    }

    // --- ALL ---

    #[test]
    fn all_contains_every_variant() {
        let every = [
            Platform::Claude,
            Platform::Copilot,
            Platform::Cursor,
            Platform::Windsurf,
            Platform::Gemini,
            Platform::Opencode,
            Platform::Codex,
            Platform::Pi,
            Platform::Crush,
            Platform::Amp,
            Platform::Zed,
            Platform::Openhands,
            Platform::Hermes,
            Platform::Devin,
        ];
        assert_eq!(Platform::ALL.len(), every.len(), "ALL must list every variant");
        for p in every {
            assert!(Platform::ALL.contains(&p), "{p} missing from Platform::ALL");
        }
    }

    #[test]
    fn all_slugs_are_unique() {
        let mut slugs: Vec<&str> = Platform::ALL.iter().map(|p| p.slug()).collect();
        let count = slugs.len();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), count, "platform slugs must be unique across ALL");
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
        assert_eq!(Platform::from_str("crush", true).unwrap(), Platform::Crush);
        assert_eq!(Platform::from_str("amp", true).unwrap(), Platform::Amp);
        assert_eq!(Platform::from_str("zed", true).unwrap(), Platform::Zed);
        assert_eq!(Platform::from_str("openhands", true).unwrap(), Platform::Openhands);
        assert_eq!(Platform::from_str("hermes", true).unwrap(), Platform::Hermes);
        assert_eq!(Platform::from_str("devin", true).unwrap(), Platform::Devin);
    }
}
