//! Shared serde types persisted in the config root and lock files: registered
//! sources ([`SourcesFile`]), global settings ([`CmxConfig`]), a scanned artifact
//! ([`Artifact`]), lock entries ([`LockFile`], [`LockEntry`]), and named artifact
//! groups ([`SetsFile`]). These are the data cmx-core reads and writes; the I/O
//! itself lives in [`crate::config`] and [`crate::lockfile`].

use crate::error::{CmxError, Result as CmxResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The persisted `sources.json` document: every registered source repo, keyed by name.
#[derive(Serialize, Deserialize)]
pub struct SourcesFile {
    /// Schema version, currently always `1`.
    pub version: u32,
    /// Registered sources, keyed by source name.
    pub sources: BTreeMap<String, SourceEntry>,
}

/// The persisted `config.json` document: cmx's global settings.
#[derive(Serialize, Deserialize)]
pub struct CmxConfig {
    /// Schema version, currently always `1`.
    pub version: u32,
    /// LLM gateway settings for `llm`-gated features (e.g. `cmx diff` analysis).
    #[serde(default)]
    pub llm: LlmConfig,
    /// Optional override for the canonical home directory that holds
    /// hand-authored artifacts. When absent, the home defaults to
    /// `<config_dir>/home` (see [`ConfigPaths::default_artifact_home`]).
    ///
    /// [`ConfigPaths::default_artifact_home`]: crate::paths::ConfigPaths::default_artifact_home
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<PathBuf>,
    /// Artifacts managed by another tool, which `cmx doctor` should report as
    /// `external` rather than flagging as orphaned/untracked. Each entry is
    /// either a **directory** (an install location, e.g. `~/.hermes/skills` —
    /// `~` expands to the OS home) or a bare **artifact name**. See
    /// [`config::matches_external`](crate::config::matches_external).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external: Vec<String>,
    /// The platforms cmx manages. When non-empty, this is the **authoritative**
    /// set: default (no `--platform`) `install`/`uninstall` act on exactly these,
    /// and `doctor` surveys only these. When empty (the default), cmx infers the
    /// set — install targets platforms already in use, while uninstall and
    /// doctor consider every supported platform. Manage with
    /// `cmx config platforms add|remove|list`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<crate::platform::Platform>,
}

impl Default for CmxConfig {
    fn default() -> Self {
        Self {
            version: 1,
            llm: LlmConfig::default(),
            home: None,
            external: Vec::new(),
            platforms: Vec::new(),
        }
    }
}

/// LLM gateway settings persisted in [`CmxConfig::llm`], used by the `llm`-gated
/// diff-analysis feature.
#[derive(Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    /// Which LLM provider to talk to.
    pub gateway: LlmGatewayType,
    /// The model identifier to request from that provider.
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            gateway: LlmGatewayType::OpenAI,
            model: "gpt-5.4".to_string(),
        }
    }
}

/// The supported LLM providers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlmGatewayType {
    /// `OpenAI`'s hosted API.
    OpenAI,
    /// A local Ollama server.
    Ollama,
}

impl std::fmt::Display for LlmGatewayType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmGatewayType::OpenAI => write!(f, "openai"),
            LlmGatewayType::Ollama => write!(f, "ollama"),
        }
    }
}

impl Default for SourcesFile {
    fn default() -> Self {
        Self {
            version: 1,
            sources: BTreeMap::new(),
        }
    }
}

/// A registered source repo — either a local directory or a git remote.
#[derive(Serialize, Deserialize, Clone)]
pub struct SourceEntry {
    /// Whether this is a [`SourceType::Local`] directory or a [`SourceType::Git`] remote.
    #[serde(rename = "type")]
    pub source_type: SourceType,
    /// The local directory path, for [`SourceType::Local`] sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The remote URL, for [`SourceType::Git`] sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Where this git source has been cloned to locally, for [`SourceType::Git`] sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_clone: Option<PathBuf>,
    /// The git branch to track, for [`SourceType::Git`] sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// RFC 3339 timestamp of the last successful `cmx source update` for this source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
}

/// Whether a [`SourceEntry`] reads from a local directory or a cloned git remote.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    /// A plain local directory, configured with `path`.
    Local,
    /// A git remote, cloned to `local_clone` and tracked via `url`/`branch`.
    Git,
}

/// An agent or skill discovered by scanning a source repo, with the metadata
/// needed to display, install, and version-track it.
#[derive(Debug, Serialize)]
pub struct Artifact {
    /// Whether this is an agent or a skill.
    pub kind: ArtifactKind,
    /// The artifact's name (file stem for agents, directory name for skills).
    pub name: String,
    /// A short human-readable description, parsed from frontmatter.
    pub description: String,
    /// The artifact's path within the source repo.
    pub path: PathBuf,
    /// The artifact's declared semver version, if any.
    pub version: Option<String>,
    /// Deprecation metadata, if the artifact's frontmatter marks it deprecated.
    pub deprecation: Option<Deprecation>,
}

/// Deprecation metadata for an [`Artifact`], parsed from frontmatter.
#[derive(Debug, Clone, Serialize)]
pub struct Deprecation {
    /// A human-readable reason the artifact is deprecated.
    pub reason: Option<String>,
    /// The name of the artifact that replaces it, if any.
    pub replacement: Option<String>,
}

/// A per-platform lock file: every artifact installed for that platform/scope,
/// keyed by name (see `cmx-core/SPEC.md` §3).
#[derive(Serialize, Deserialize)]
pub struct LockFile {
    /// Schema version, currently always `1`.
    pub version: u32,
    /// Installed artifacts, keyed by artifact name.
    pub packages: BTreeMap<String, LockEntry>,
}

impl Default for LockFile {
    fn default() -> Self {
        Self {
            version: 1,
            packages: BTreeMap::new(),
        }
    }
}

/// A single artifact's entry in a [`LockFile`] — what was installed, from where,
/// and its checksums (see `cmx-core/SPEC.md` §3.1).
#[derive(Serialize, Deserialize, Clone)]
pub struct LockEntry {
    /// Whether the tracked artifact is an agent or a skill.
    #[serde(rename = "type")]
    pub artifact_type: ArtifactKind,
    /// The installed version string, when the artifact declares one. Omitted
    /// entirely (not `null`) when absent, per the lock-file contract.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// RFC 3339 timestamp of when this entry was written.
    pub installed_at: String,
    /// Where the artifact came from.
    pub source: LockSource,
    /// The checksum of the artifact at the source, at install time.
    pub source_checksum: String,
    /// The checksum of the artifact as installed on disk.
    pub installed_checksum: String,
}

impl LockEntry {
    /// Canonical constructor — prefer this over struct literal syntax so all
    /// creation sites update when the struct grows.
    pub fn new(
        kind: ArtifactKind,
        version: Option<String>,
        source: LockSource,
        source_checksum: String,
        installed_checksum: String,
        installed_at: String,
    ) -> Self {
        Self {
            artifact_type: kind,
            version,
            installed_at,
            source,
            source_checksum,
            installed_checksum,
        }
    }
}

/// An artifact found installed on disk, paired with its lock entry (if tracked)
/// and its version as read back from the installed content.
pub struct InstalledArtifact<'a> {
    /// The artifact's name.
    pub name: String,
    /// The matching entry in the platform's lock file, if this artifact is tracked.
    pub lock_entry: Option<&'a LockEntry>,
    /// The version parsed from the installed content itself (frontmatter), which
    /// may disagree with `lock_entry`'s version if the two have drifted.
    pub installed_version: Option<String>,
}

/// Where a [`LockEntry`]'s artifact came from — a source name and its path within
/// that source.
#[derive(Serialize, Deserialize, Clone)]
pub struct LockSource {
    /// The source name (or `bundled:<name>` for a tool's own bundled skill).
    pub repo: String,
    /// The artifact's path within that source.
    pub path: String,
}

impl LockSource {
    /// Construct a `LockSource` from a repo name and path.
    pub fn new(repo: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            path: path.into(),
        }
    }
}

/// Whether an artifact is installed user-wide (`$HOME`) or project-local (the
/// current working directory), which selects both the install directory and the
/// lock file location (see `cmx-core/SPEC.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallScope {
    /// User-wide: anchored at the OS home directory.
    Global,
    /// Project-local: anchored at the current project's working directory.
    Local,
}

impl InstallScope {
    /// The lowercase display label (`"global"` or `"local"`).
    pub fn label(&self) -> &'static str {
        match self {
            InstallScope::Global => "global",
            InstallScope::Local => "local",
        }
    }

    /// Whether this scope is [`InstallScope::Local`].
    pub fn is_local(&self) -> bool {
        matches!(self, InstallScope::Local)
    }

    /// Both scopes, global first — the canonical iteration order used when a
    /// command considers all scopes.
    pub const ALL: [InstallScope; 2] = [InstallScope::Global, InstallScope::Local];
}

/// Whether an artifact is a file-droppable agent or a directory-based skill —
/// the two kinds cmx-core installs and tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// A single file (markdown, or TOML for Codex) dropped into a platform's
    /// agent directory.
    Agent,
    /// A directory named after the artifact, containing `SKILL.md` and any
    /// bundled files.
    Skill,
}

impl std::fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactKind::Agent => write!(f, "agent"),
            ArtifactKind::Skill => write!(f, "skill"),
        }
    }
}

impl ArtifactKind {
    /// The agent file extension used in the canonical, tool-neutral home.
    ///
    /// The home predates any platform-specific projection, so home agents are
    /// always plain markdown regardless of which tools they're later installed
    /// for. Pass this to [`installed_path`](Self::installed_path) when building a
    /// home path.
    pub const HOME_AGENT_EXT: &'static str = "md";

    /// Compute the expected filesystem path for an installed artifact within a
    /// given install directory.
    ///
    /// `agent_ext` is the platform's agent file extension (e.g. `md`, or `toml`
    /// for codex). It is ignored for skills, which install as a directory named
    /// after the artifact. Pass [`HOME_AGENT_EXT`](Self::HOME_AGENT_EXT) when the
    /// target is the canonical home.
    pub fn installed_path(&self, name: &str, dir: &Path, agent_ext: &str) -> PathBuf {
        match self {
            ArtifactKind::Agent => dir.join(format!("{name}.{agent_ext}")),
            ArtifactKind::Skill => dir.join(name),
        }
    }

    /// Determine whether a directory entry represents a valid installed artifact
    /// for this kind, returning the artifact name if it matches.
    ///
    /// `agent_ext` is the platform's agent file extension (e.g. `md`, or `toml`
    /// for codex), so that codex's TOML agents are recognized as well as
    /// markdown ones.
    pub fn artifact_name_from_entry(
        &self,
        entry: &crate::gateway::filesystem::DirEntry,
        agent_ext: &str,
    ) -> Option<String> {
        match self {
            ArtifactKind::Agent => {
                let path = Path::new(&entry.file_name);
                path.extension()
                    .filter(|ext| ext.eq_ignore_ascii_case(agent_ext))
                    .and_then(|_| path.file_stem())
                    .map(|stem| stem.to_string_lossy().to_string())
            }
            ArtifactKind::Skill => entry.is_dir.then(|| entry.file_name.clone()),
        }
    }

    /// Return the subdirectory name used by this kind in a plugin source tree.
    pub fn subdir_name(&self) -> &'static str {
        match self {
            ArtifactKind::Agent => "agents",
            ArtifactKind::Skill => "skills",
        }
    }

    /// Return the path to the content file for an artifact of this kind given
    /// its base path.
    ///
    /// For agents the base path is the `.md` file itself; for skills it is the
    /// directory that contains `SKILL.md`.
    pub fn content_path(&self, base: &Path) -> PathBuf {
        match self {
            ArtifactKind::Agent => base.to_path_buf(),
            ArtifactKind::Skill => base.join("SKILL.md"),
        }
    }

    /// Derive an artifact name from its source path.
    ///
    /// For agents the name is the file stem (strips the `.md` extension).
    /// For skills the name is the directory name as-is.
    pub fn artifact_name_from_path(&self, path: &Path) -> Option<String> {
        match self {
            ArtifactKind::Agent => path.file_stem().map(|s| s.to_string_lossy().to_string()),
            ArtifactKind::Skill => path.file_name().map(|s| s.to_string_lossy().to_string()),
        }
    }
}

impl Artifact {
    /// Whether this artifact's frontmatter marks it as deprecated.
    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }
}

/// Return `path` relative to `base` as a `String`, falling back to the full path if
/// `path` does not start with `base`.
pub fn relative_path_string(path: &Path, base: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).to_string_lossy().to_string()
}

/// Render an optional version for display, substituting `"-"` when absent.
pub fn display_version(v: Option<&str>) -> &str {
    v.unwrap_or("-")
}

/// Render an optional version as a leading `" vX.Y.Z"` suffix for display, or an
/// empty string when absent.
pub fn format_version_prefix(version: Option<&str>) -> String {
    version.map(|v| format!(" v{v}")).unwrap_or_default()
}

impl SourcesFile {
    /// Look up a registered source by name, or [`CmxError::SourceNotFound`] if it
    /// is not registered.
    pub fn get_source(&self, name: &str) -> CmxResult<&SourceEntry> {
        self.sources.get(name).ok_or_else(|| CmxError::SourceNotFound {
            name: name.to_string(),
        })
    }
}

/// Sets state file (`sets.json`) — a locally-defined, named group of installed
/// artifacts with a desired activation state. See `SETS.md` for the full design.
/// Phase 1 covers definitions and curation only; `activate`/`deactivate` are
/// Phase 2.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SetsFile {
    /// Schema version, currently always `1`.
    pub version: u32,
    /// Defined sets, keyed by set name.
    pub sets: BTreeMap<String, SetDef>,
}

impl Default for SetsFile {
    fn default() -> Self {
        Self {
            version: 1,
            sets: BTreeMap::new(),
        }
    }
}

/// A single named set: a curated group of installed artifacts with a desired
/// activation state.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SetDef {
    /// An optional human-readable description of what this set is for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the set's members are currently installed (activated) or not.
    pub state: SetState,
    /// The artifacts that belong to this set.
    pub members: Vec<SetMember>,
}

/// Whether a [`SetDef`]'s members are installed (`Active`) or not (`Inactive`).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SetState {
    /// The set's members are installed.
    Active,
    /// The set's members are not installed.
    Inactive,
}

impl std::fmt::Display for SetState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetState::Active => write!(f, "active"),
            SetState::Inactive => write!(f, "inactive"),
        }
    }
}

/// A single artifact tracked as a member of a set. `source` is the source
/// repo name, snapshotted from the lockfile at `set add` time (see
/// `SETS.md`, "The source pin") so `activate` (Phase 2) can re-install
/// deterministically even after the lock entry is gone.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SetMember {
    /// Whether the member is an agent or a skill.
    #[serde(rename = "type")]
    pub kind: ArtifactKind,
    /// The member artifact's name.
    pub name: String,
    /// The source repo name this member was installed from, snapshotted at
    /// `set add` time so it survives the source or lock entry disappearing later.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{make_git_entry, make_local_entry, sample_lock_file};
    use std::collections::BTreeMap;

    #[test]
    fn lockfile_round_trip() {
        let lock = sample_lock_file();
        let json = serde_json::to_string(&lock).expect("serialize");
        let restored: LockFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.version, 1);
        let entry = restored.packages.get("my-agent").expect("entry present");
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
        assert_eq!(entry.source.repo, "guidelines");
        assert_eq!(entry.installed_checksum, "sha256:def456");
    }

    #[test]
    fn lockfile_artifact_type_serializes_as_agent() {
        let lock = sample_lock_file();
        let json = serde_json::to_string(&lock).expect("serialize");
        // ArtifactKind::Agent with rename_all="lowercase" should serialize as "agent"
        assert!(json.contains("\"agent\""), "expected \"agent\" in JSON: {json}");
    }

    #[test]
    fn lockfile_optional_version_omitted_when_none() {
        let mut lock = sample_lock_file();
        lock.packages.get_mut("my-agent").unwrap().version = None;
        let json = serde_json::to_string(&lock).expect("serialize");
        // The per-entry "version" field should be absent when None.
        // We parse back to verify: the restored entry has no version.
        let restored: LockFile = serde_json::from_str(&json).expect("deserialize");
        let entry = restored.packages.get("my-agent").expect("entry present");
        assert!(entry.version.is_none(), "version should be None after round-trip");
    }

    // --- SourcesFile round-trip ---

    #[test]
    fn sources_file_round_trip_local() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "local-source".to_string(),
            make_local_entry(
                "/home/user/repos/guidelines",
                Some("2024-01-01T00:00:00Z".to_string()),
            ),
        );
        let sf = SourcesFile {
            version: 1,
            sources,
        };
        let json = serde_json::to_string(&sf).expect("serialize");
        let restored: SourcesFile = serde_json::from_str(&json).expect("deserialize");
        let entry = restored.sources.get("local-source").expect("entry");
        assert!(matches!(entry.source_type, SourceType::Local));
        assert!(entry.url.is_none());
    }

    #[test]
    fn sources_file_type_field_serializes_correctly() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "git-source".to_string(),
            make_git_entry("https://github.com/example/repo", "/tmp/repo", "main", None),
        );
        let sf = SourcesFile {
            version: 1,
            sources,
        };
        let json = serde_json::to_string(&sf).expect("serialize");
        // SourceType::Git with rename_all="lowercase" must produce "git"
        assert!(json.contains("\"git\""), "expected \"git\" in JSON: {json}");
    }

    // --- CmxConfig round-trip ---

    #[test]
    fn cmx_config_default_round_trip() {
        let config = CmxConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let restored: CmxConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.version, config.version);
        assert_eq!(restored.llm.model, config.llm.model);
    }

    // --- ArtifactKind Display ---

    #[test]
    fn artifact_kind_display_agent() {
        assert_eq!(format!("{}", ArtifactKind::Agent), "agent");
    }

    #[test]
    fn artifact_kind_display_skill() {
        assert_eq!(format!("{}", ArtifactKind::Skill), "skill");
    }

    // --- ArtifactKind installed_path ---

    #[test]
    fn installed_path_agent_appends_md_extension() {
        let dir = Path::new("/home/user/.claude/agents");
        let path =
            ArtifactKind::Agent.installed_path("my-agent", dir, ArtifactKind::HOME_AGENT_EXT);
        assert_eq!(path, PathBuf::from("/home/user/.claude/agents/my-agent.md"));
    }

    #[test]
    fn installed_path_skill_uses_bare_name() {
        let dir = Path::new("/home/user/.claude/skills");
        let path =
            ArtifactKind::Skill.installed_path("my-skill", dir, ArtifactKind::HOME_AGENT_EXT);
        assert_eq!(path, PathBuf::from("/home/user/.claude/skills/my-skill"));
    }

    // --- Artifact accessors ---

    fn make_agent() -> Artifact {
        Artifact {
            kind: ArtifactKind::Agent,
            name: "test-agent".to_string(),
            description: "Agent description".to_string(),
            path: PathBuf::from("test-agent.md"),
            version: Some("2.0.0".to_string()),
            deprecation: None,
        }
    }

    fn make_skill() -> Artifact {
        Artifact {
            kind: ArtifactKind::Skill,
            name: "test-skill".to_string(),
            description: "Skill description".to_string(),
            path: PathBuf::from("test-skill"),
            version: Some("1.0.0".to_string()),
            deprecation: Some(Deprecation {
                reason: Some("Old".to_string()),
                replacement: Some("new-skill".to_string()),
            }),
        }
    }

    #[test]
    fn artifact_agent_accessors() {
        let a = make_agent();
        assert_eq!(a.name, "test-agent");
        assert_eq!(a.description, "Agent description");
        assert_eq!(a.kind.to_string(), "agent");
        assert_eq!(a.kind, ArtifactKind::Agent);
        assert_eq!(a.path, PathBuf::from("test-agent.md"));
        assert_eq!(a.version.as_deref(), Some("2.0.0"));
        assert!(!a.is_deprecated());
        assert!(a.deprecation.is_none());
    }

    #[test]
    fn artifact_skill_accessors() {
        let s = make_skill();
        assert_eq!(s.name, "test-skill");
        assert_eq!(s.description, "Skill description");
        assert_eq!(s.kind.to_string(), "skill");
        assert_eq!(s.kind, ArtifactKind::Skill);
        assert_eq!(s.path, PathBuf::from("test-skill"));
        assert_eq!(s.version.as_deref(), Some("1.0.0"));
        assert!(s.is_deprecated());
        let dep = s.deprecation.as_ref().unwrap();
        assert_eq!(dep.reason.as_deref(), Some("Old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-skill"));
    }

    // --- ArtifactKind::subdir_name ---

    #[test]
    fn subdir_name_agent() {
        assert_eq!(ArtifactKind::Agent.subdir_name(), "agents");
    }

    #[test]
    fn subdir_name_skill() {
        assert_eq!(ArtifactKind::Skill.subdir_name(), "skills");
    }

    // --- ArtifactKind::content_path ---

    #[test]
    fn content_path_agent_returns_base_unchanged() {
        let base = Path::new("/repo/agents/my-agent.md");
        assert_eq!(
            ArtifactKind::Agent.content_path(base),
            PathBuf::from("/repo/agents/my-agent.md")
        );
    }

    #[test]
    fn content_path_skill_appends_skill_md() {
        let base = Path::new("/repo/my-skill");
        assert_eq!(
            ArtifactKind::Skill.content_path(base),
            PathBuf::from("/repo/my-skill/SKILL.md")
        );
    }

    // --- ArtifactKind::artifact_name_from_path ---

    #[test]
    fn artifact_name_from_path_agent_strips_extension() {
        let path = Path::new("/repo/agents/rust-craftsperson.md");
        assert_eq!(
            ArtifactKind::Agent.artifact_name_from_path(path),
            Some("rust-craftsperson".to_string())
        );
    }

    #[test]
    fn artifact_name_from_path_skill_uses_dir_name() {
        let path = Path::new("/repo/my-skill");
        assert_eq!(ArtifactKind::Skill.artifact_name_from_path(path), Some("my-skill".to_string()));
    }

    // --- Golden JSON test ---

    #[test]
    fn lockfile_golden_json_parses_correctly() {
        let json = r#"{
            "version": 1,
            "packages": {
                "rust-craftsperson": {
                    "type": "agent",
                    "version": "3.1.0",
                    "installed_at": "2024-06-01T12:00:00+00:00",
                    "source": {
                        "repo": "guidelines",
                        "path": "agents/rust-craftsperson.md"
                    },
                    "source_checksum": "sha256:aabbcc",
                    "installed_checksum": "sha256:ddeeff"
                }
            }
        }"#;
        let lock: LockFile = serde_json::from_str(json).expect("golden JSON must parse");
        assert_eq!(lock.version, 1);
        let entry = lock.packages.get("rust-craftsperson").expect("entry present");
        assert_eq!(entry.artifact_type, ArtifactKind::Agent);
        assert_eq!(entry.version.as_deref(), Some("3.1.0"));
        assert_eq!(entry.source.repo, "guidelines");
        assert_eq!(entry.source_checksum, "sha256:aabbcc");
    }

    // --- SetsFile golden JSON round-trip ---

    const SETS_EXAMPLE_JSON: &str = r#"{
        "version": 1,
        "sets": {
            "rust-work": {
                "description": "Rust craftsmanship + foundry",
                "state": "active",
                "members": [
                    { "type": "agent", "name": "rust-craftsperson", "source": "guidelines" },
                    { "type": "skill", "name": "foundry", "source": "home" }
                ]
            },
            "client-ort": {
                "state": "inactive",
                "members": [
                    { "type": "skill", "name": "ubiquity-router", "source": "home" }
                ]
            }
        }
    }"#;

    #[test]
    fn sets_file_golden_json_round_trips_exactly() {
        let parsed: SetsFile =
            serde_json::from_str(SETS_EXAMPLE_JSON).expect("golden JSON must parse");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.sets.len(), 2);

        let rust_work = parsed.sets.get("rust-work").expect("rust-work present");
        assert_eq!(rust_work.description.as_deref(), Some("Rust craftsmanship + foundry"));
        assert_eq!(rust_work.state, SetState::Active);
        assert_eq!(rust_work.members.len(), 2);
        assert_eq!(rust_work.members[0].kind, ArtifactKind::Agent);
        assert_eq!(rust_work.members[0].name, "rust-craftsperson");
        assert_eq!(rust_work.members[0].source.as_deref(), Some("guidelines"));

        let client_ort = parsed.sets.get("client-ort").expect("client-ort present");
        assert!(client_ort.description.is_none());
        assert_eq!(client_ort.state, SetState::Inactive);

        // Round-trip via Value comparison: order/whitespace-independent, but
        // proves the exact key set (version/sets/description/state/members/
        // type/name/source) matches the SETS.md example.
        let expected: serde_json::Value =
            serde_json::from_str(SETS_EXAMPLE_JSON).expect("expected JSON parses");
        let actual = serde_json::to_value(&parsed).expect("serialize");
        assert_eq!(actual, expected);
    }

    #[test]
    fn set_state_serializes_lowercase() {
        let json = serde_json::to_string(&SetState::Active).unwrap();
        assert_eq!(json, "\"active\"");
        let json = serde_json::to_string(&SetState::Inactive).unwrap();
        assert_eq!(json, "\"inactive\"");
    }

    #[test]
    fn set_member_kind_serializes_under_type_key() {
        let member = SetMember {
            kind: ArtifactKind::Agent,
            name: "rust-craftsperson".to_string(),
            source: Some("guidelines".to_string()),
        };
        let json = serde_json::to_string(&member).unwrap();
        assert!(json.contains("\"type\":\"agent\""), "expected type key: {json}");
        assert!(!json.contains("\"kind\""), "kind should not appear literally: {json}");
    }

    #[test]
    fn set_def_omits_description_when_none() {
        let def = SetDef {
            description: None,
            state: SetState::Inactive,
            members: vec![],
        };
        let json = serde_json::to_string(&def).unwrap();
        assert!(!json.contains("description"), "description should be omitted: {json}");
        assert!(
            json.contains("\"members\":[]"),
            "members should serialize even when empty: {json}"
        );
    }

    #[test]
    fn set_member_omits_source_when_none() {
        let member = SetMember {
            kind: ArtifactKind::Skill,
            name: "foo".to_string(),
            source: None,
        };
        let json = serde_json::to_string(&member).unwrap();
        assert!(!json.contains("source"), "source should be omitted: {json}");
    }

    #[test]
    fn sets_file_default_is_empty_version_one() {
        let sets = SetsFile::default();
        assert_eq!(sets.version, 1);
        assert!(sets.sets.is_empty());
    }
}
