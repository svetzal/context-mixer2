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
pub enum Artifact {
    Agent {
        name: String,
        description: String,
        path: PathBuf,
        version: Option<String>,
        deprecation: Option<Deprecation>,
    },
    Skill {
        name: String,
        description: String,
        path: PathBuf,
        version: Option<String>,
        deprecation: Option<Deprecation>,
    },
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

#[derive(Serialize, Deserialize, Clone)]
pub struct LockEntry {
    #[serde(rename = "type")]
    pub artifact_type: ArtifactKindSerde,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKindSerde {
    Agent,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl Artifact {
    pub fn name(&self) -> &str {
        match self {
            Artifact::Agent { name, .. } => name,
            Artifact::Skill { name, .. } => name,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Artifact::Agent { description, .. } => description,
            Artifact::Skill { description, .. } => description,
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            Artifact::Agent { .. } => "agent",
            Artifact::Skill { .. } => "skill",
        }
    }

    pub fn artifact_kind(&self) -> ArtifactKind {
        match self {
            Artifact::Agent { .. } => ArtifactKind::Agent,
            Artifact::Skill { .. } => ArtifactKind::Skill,
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Artifact::Agent { path, .. } => path,
            Artifact::Skill { path, .. } => path,
        }
    }

    pub fn version(&self) -> Option<&str> {
        match self {
            Artifact::Agent { version, .. } => version.as_deref(),
            Artifact::Skill { version, .. } => version.as_deref(),
        }
    }

    pub fn deprecation(&self) -> Option<&Deprecation> {
        match self {
            Artifact::Agent { deprecation, .. } => deprecation.as_ref(),
            Artifact::Skill { deprecation, .. } => deprecation.as_ref(),
        }
    }

    pub fn is_deprecated(&self) -> bool {
        self.deprecation().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // --- LockFile round-trip ---

    fn sample_lock_file() -> LockFile {
        let mut packages = BTreeMap::new();
        packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKindSerde::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: "2024-01-01T00:00:00Z".to_string(),
                source: LockSource {
                    repo: "guidelines".to_string(),
                    path: "agents/my-agent.md".to_string(),
                },
                source_checksum: "sha256:abc123".to_string(),
                installed_checksum: "sha256:def456".to_string(),
            },
        );
        LockFile {
            version: 1,
            packages,
        }
    }

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
        // ArtifactKindSerde::Agent with rename_all="lowercase" should serialize as "agent"
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
            SourceEntry {
                source_type: SourceType::Local,
                path: Some(PathBuf::from("/home/user/repos/guidelines")),
                url: None,
                local_clone: None,
                branch: None,
                last_updated: Some("2024-01-01T00:00:00Z".to_string()),
            },
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
            SourceEntry {
                source_type: SourceType::Git,
                path: None,
                url: Some("https://github.com/example/repo".to_string()),
                local_clone: Some(PathBuf::from("/tmp/repo")),
                branch: Some("main".to_string()),
                last_updated: None,
            },
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

    // --- Artifact accessors ---

    fn make_agent() -> Artifact {
        Artifact::Agent {
            name: "test-agent".to_string(),
            description: "Agent description".to_string(),
            path: PathBuf::from("test-agent.md"),
            version: Some("2.0.0".to_string()),
            deprecation: None,
        }
    }

    fn make_skill() -> Artifact {
        Artifact::Skill {
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
        assert_eq!(a.name(), "test-agent");
        assert_eq!(a.description(), "Agent description");
        assert_eq!(a.kind(), "agent");
        assert_eq!(a.artifact_kind(), ArtifactKind::Agent);
        assert_eq!(a.path(), std::path::Path::new("test-agent.md"));
        assert_eq!(a.version(), Some("2.0.0"));
        assert!(!a.is_deprecated());
        assert!(a.deprecation().is_none());
    }

    #[test]
    fn artifact_skill_accessors() {
        let s = make_skill();
        assert_eq!(s.name(), "test-skill");
        assert_eq!(s.description(), "Skill description");
        assert_eq!(s.kind(), "skill");
        assert_eq!(s.artifact_kind(), ArtifactKind::Skill);
        assert_eq!(s.path(), std::path::Path::new("test-skill"));
        assert_eq!(s.version(), Some("1.0.0"));
        assert!(s.is_deprecated());
        let dep = s.deprecation().unwrap();
        assert_eq!(dep.reason.as_deref(), Some("Old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-skill"));
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
        assert!(matches!(entry.artifact_type, ArtifactKindSerde::Agent));
        assert_eq!(entry.version.as_deref(), Some("3.1.0"));
        assert_eq!(entry.source.repo, "guidelines");
        assert_eq!(entry.source_checksum, "sha256:aabbcc");
    }
}
