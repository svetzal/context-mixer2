use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config;
use crate::scan;
use crate::types::{Artifact, SourceEntry};

/// An artifact discovered during source scanning, with its source context.
pub struct SourceArtifact {
    pub source_name: String,
    pub source_root: PathBuf,
    pub artifact: Artifact,
}

/// Iterate over all artifacts across a set of registered sources.
///
/// Resolves local paths, scans each source, and returns every artifact found
/// with its source context. Silently skips sources whose local paths do not
/// exist or that fail to scan (sources may be unavailable during normal use).
pub fn each_source_artifact(sources: &BTreeMap<String, SourceEntry>) -> Vec<SourceArtifact> {
    let mut results = Vec::new();

    for (source_name, entry) in sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in artifacts {
                results.push(SourceArtifact {
                    source_name: source_name.clone(),
                    source_root: local_path.clone(),
                    artifact,
                });
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArtifactKind, SourceType};
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    fn make_local_entry(path: std::path::PathBuf) -> SourceEntry {
        SourceEntry {
            source_type: SourceType::Local,
            path: Some(path),
            url: None,
            local_clone: None,
            branch: None,
            last_updated: None,
        }
    }

    #[test]
    fn each_source_artifact_skips_missing_paths() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "missing".to_string(),
            make_local_entry(std::path::PathBuf::from("/nonexistent/path/that/does/not/exist")),
        );

        let results = each_source_artifact(&sources);
        assert!(results.is_empty(), "should yield no results for a missing path");
    }

    #[test]
    fn each_source_artifact_finds_artifacts() {
        let dir = TempDir::new().unwrap();
        // Write a valid agent file
        let agent_content = "---\nname: my-agent\ndescription: Test agent\n---\n\n# my-agent\n";
        fs::write(dir.path().join("my-agent.md"), agent_content).unwrap();

        let mut sources = BTreeMap::new();
        sources.insert("test-source".to_string(), make_local_entry(dir.path().to_path_buf()));

        let results = each_source_artifact(&sources);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "test-source");
        assert_eq!(results[0].artifact.name, "my-agent");
        assert_eq!(results[0].artifact.kind, ArtifactKind::Agent);
    }
}
