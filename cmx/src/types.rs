use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
pub struct SourcesFile {
    pub version: u32,
    pub sources: BTreeMap<String, SourceEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct CmxConfig {
    pub version: u32,
    #[serde(default)]
    pub llm: LlmConfig,
}

impl Default for CmxConfig {
    fn default() -> Self {
        Self {
            version: 1,
            llm: LlmConfig::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    pub gateway: LlmGatewayType,
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlmGatewayType {
    OpenAI,
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

#[derive(Serialize, Deserialize, Clone)]
pub struct SourceEntry {
    #[serde(rename = "type")]
    pub source_type: SourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_clone: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Local,
    Git,
}

#[derive(Debug)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub version: Option<String>,
    pub deprecation: Option<Deprecation>,
}

#[derive(Debug, Clone)]
pub struct Deprecation {
    pub reason: Option<String>,
    pub replacement: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct LockFile {
    pub version: u32,
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

#[derive(Serialize, Deserialize, Clone)]
pub struct LockEntry {
    #[serde(rename = "type")]
    pub artifact_type: ArtifactKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub installed_at: String,
    pub source: LockSource,
    pub source_checksum: String,
    pub installed_checksum: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LockSource {
    pub repo: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Agent,
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
    /// Compute the expected filesystem path for an installed artifact within a
    /// given install directory.
    pub fn installed_path(&self, name: &str, dir: &Path) -> PathBuf {
        match self {
            ArtifactKind::Agent => dir.join(format!("{name}.md")),
            ArtifactKind::Skill => dir.join(name),
        }
    }

    /// Remove an installed artifact from disk, dispatching to the correct
    /// removal strategy based on kind: file removal for agents, recursive
    /// directory removal for skills.
    pub fn remove_installed(
        &self,
        path: &Path,
        fs: &dyn crate::gateway::filesystem::Filesystem,
    ) -> anyhow::Result<()> {
        match self {
            ArtifactKind::Agent => fs.remove_file(path)?,
            ArtifactKind::Skill => fs.remove_dir_all(path)?,
        }
        Ok(())
    }

    /// Determine whether a directory entry represents a valid installed artifact
    /// for this kind, returning the artifact name if it matches.
    pub fn artifact_name_from_entry(
        &self,
        entry: &crate::gateway::filesystem::DirEntry,
    ) -> Option<String> {
        match self {
            ArtifactKind::Agent => Path::new(&entry.file_name)
                .extension()
                .filter(|ext| ext.eq_ignore_ascii_case("md"))
                .map(|_| entry.file_name.trim_end_matches(".md").to_string()),
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

    /// Produce a textual diff between an installed artifact and its source
    /// counterpart, dispatching to the correct strategy (file diff for agents,
    /// directory diff for skills).
    #[cfg(feature = "llm")]
    pub fn diff_with(
        &self,
        installed: &Path,
        source: &Path,
        ctx: &crate::context::AppContext<'_>,
    ) -> anyhow::Result<String> {
        match self {
            ArtifactKind::Agent => crate::diff::diff_files_with(installed, source, ctx),
            ArtifactKind::Skill => crate::diff::diff_dirs_with(installed, source, ctx),
        }
    }

    /// Copy an artifact from `source` into `dest_dir`, dispatching to the
    /// correct strategy (file copy for agents, recursive directory copy for
    /// skills). Returns the destination path.
    pub fn copy_to(
        &self,
        source: &Path,
        dest_dir: &Path,
        fs: &dyn crate::gateway::filesystem::Filesystem,
    ) -> anyhow::Result<PathBuf> {
        let name = source
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid source path: {}", source.display()))?;
        let dest = dest_dir.join(name);
        match self {
            ArtifactKind::Agent => {
                fs.copy_file(source, &dest).with_context(|| {
                    format!("Failed to copy {} to {}", source.display(), dest.display())
                })?;
            }
            ArtifactKind::Skill => {
                crate::copy::copy_dir_recursive_with(source, &dest, fs)?;
            }
        }
        Ok(dest)
    }
}

impl Artifact {
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
        let path = ArtifactKind::Agent.installed_path("my-agent", dir);
        assert_eq!(path, PathBuf::from("/home/user/.claude/agents/my-agent.md"));
    }

    #[test]
    fn installed_path_skill_uses_bare_name() {
        let dir = Path::new("/home/user/.claude/skills");
        let path = ArtifactKind::Skill.installed_path("my-skill", dir);
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
}
