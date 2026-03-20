use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct SourcesFile {
    pub version: u32,
    pub sources: BTreeMap<String, SourceEntry>,
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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Local,
    Git,
}

#[derive(Debug)]
pub enum Artifact {
    Agent { name: String, path: PathBuf },
    Skill { name: String, path: PathBuf },
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
}
