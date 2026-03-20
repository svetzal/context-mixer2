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
        path: PathBuf,
        version: Option<String>,
        deprecation: Option<Deprecation>,
    },
    Skill {
        name: String,
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
