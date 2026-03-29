use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::checksum;
use crate::config;
use crate::gateway::filesystem::Filesystem;
use crate::scan;
use crate::types::{Artifact, SourceEntry};

/// Checksum and metadata for a source artifact, used for update/outdated comparisons.
pub struct SourceArtifactInfo {
    pub source_name: String,
    pub version: Option<String>,
    pub checksum: String,
}

/// Scan all registered sources for all artifact kinds, computing a checksum for
/// each.  Returns a map keyed by artifact name.  When the same name appears in
/// multiple sources the last one wins (consistent with the existing behaviour in
/// `outdated.rs`).
pub fn scan_all_with_checksums(
    sources: &BTreeMap<String, SourceEntry>,
    fs: &dyn Filesystem,
) -> Result<BTreeMap<String, SourceArtifactInfo>> {
    let mut result = BTreeMap::new();

    for sa in each_source_artifact_with(sources, fs) {
        let cs = checksum::checksum_artifact_with(&sa.artifact.path, sa.artifact.kind, fs)?;
        result.insert(
            sa.artifact.name,
            SourceArtifactInfo {
                source_name: sa.source_name,
                version: sa.artifact.version,
                checksum: cs,
            },
        );
    }

    Ok(result)
}

/// An artifact discovered during source scanning, with its source context.
pub struct SourceArtifact {
    pub source_name: String,
    pub source_root: PathBuf,
    pub artifact: Artifact,
}

/// Iterate over all artifacts across a set of registered sources via the given filesystem.
///
/// Resolves local paths, scans each source, and returns every artifact found
/// with its source context. Silently skips sources whose local paths do not
/// exist or that fail to scan (sources may be unavailable during normal use).
pub fn each_source_artifact_with(
    sources: &BTreeMap<String, SourceEntry>,
    fs: &dyn Filesystem,
) -> Vec<SourceArtifact> {
    let mut results = Vec::new();

    for (source_name, entry) in sources {
        let local_path = config::resolve_local_path(entry);
        if !fs.exists(&local_path) {
            continue;
        }
        if let Ok(scan_result) = scan::scan_source_with(&local_path, fs) {
            for artifact in scan_result.artifacts {
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
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{agent_content, make_local_entry};
    use crate::types::ArtifactKind;
    use std::collections::BTreeMap;

    #[test]
    fn each_source_artifact_skips_missing_paths() {
        let fs = FakeFilesystem::new();
        let mut sources = BTreeMap::new();
        sources.insert(
            "missing".to_string(),
            make_local_entry("/nonexistent/path/that/does/not/exist", None),
        );

        let results = each_source_artifact_with(&sources, &fs);
        assert!(results.is_empty(), "should yield no results for a missing path");
    }

    #[test]
    fn each_source_artifact_finds_artifacts() {
        let fs = FakeFilesystem::new();
        fs.add_file("/test-repo/my-agent.md", agent_content("my-agent", "Test agent"));

        let mut sources = BTreeMap::new();
        sources.insert("test-source".to_string(), make_local_entry("/test-repo", None));

        let results = each_source_artifact_with(&sources, &fs);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_name, "test-source");
        assert_eq!(results[0].artifact.name, "my-agent");
        assert_eq!(results[0].artifact.kind, ArtifactKind::Agent);
    }
}
