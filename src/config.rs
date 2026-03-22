use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::gateway::filesystem::Filesystem;
use crate::paths::ConfigPaths;
use crate::types::{ArtifactKind, CmxConfig, SourceEntry, SourceType, SourcesFile};

// ---------------------------------------------------------------------------
// Testable variants (accept injected Filesystem + ConfigPaths)
// ---------------------------------------------------------------------------

pub fn load_sources_with(fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<SourcesFile> {
    let path = paths.sources_path();
    if !fs.exists(&path) {
        return Ok(SourcesFile::default());
    }
    let content = fs
        .read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let sources: SourcesFile = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(sources)
}

pub fn save_sources_with(
    sources: &SourcesFile,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<()> {
    let path = paths.sources_path();
    if let Some(parent) = path.parent() {
        fs.create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(sources)?;
    fs.write(&path, &content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn load_config_with(fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<CmxConfig> {
    let path = paths.config_path();
    if !fs.exists(&path) {
        return Ok(CmxConfig::default());
    }
    let content = fs
        .read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let config: CmxConfig = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(config)
}

pub fn save_config_with(
    config: &CmxConfig,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<()> {
    let path = paths.config_path();
    if let Some(parent) = path.parent() {
        fs.create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs.write(&path, &content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn installed_names_with(
    kind: ArtifactKind,
    local: bool,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<String>> {
    let dir = paths.install_dir(kind, local);
    if !fs.exists(&dir) {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs.read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        if entry.file_name.starts_with('.') {
            continue;
        }

        match kind {
            ArtifactKind::Agent => {
                if std::path::Path::new(&entry.file_name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                {
                    names.push(entry.file_name.trim_end_matches(".md").to_string());
                }
            }
            ArtifactKind::Skill => {
                if entry.is_dir {
                    names.push(entry.file_name.clone());
                }
            }
        }
    }

    names.sort();
    Ok(names)
}

pub fn resolve_local_path(entry: &SourceEntry) -> PathBuf {
    match entry.source_type {
        SourceType::Local => entry.path.clone().unwrap_or_default(),
        SourceType::Git => entry.local_clone.clone().unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Unit tests (use FakeFilesystem + ConfigPaths::for_test)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::test_paths;
    use crate::types::{SourceEntry, SourceType};
    use std::path::PathBuf;

    // --- load_sources_with ---

    #[test]
    fn load_sources_returns_default_when_file_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources = load_sources_with(&fs, &paths).unwrap();
        assert!(sources.sources.is_empty());
        assert_eq!(sources.version, 1);
    }

    #[test]
    fn load_sources_parses_valid_json() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let json = r#"{"version":1,"sources":{"my-source":{"type":"local","path":"/some/path","last_updated":"2024-01-01T00:00:00Z"}}}"#;
        fs.add_file(paths.sources_path(), json);
        let sources = load_sources_with(&fs, &paths).unwrap();
        assert!(sources.sources.contains_key("my-source"));
    }

    #[test]
    fn load_sources_returns_error_on_malformed_json() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        fs.add_file(paths.sources_path(), "not valid json{{{{");
        let result = load_sources_with(&fs, &paths);
        assert!(result.is_err());
    }

    #[test]
    fn save_sources_creates_parent_dirs_and_writes_json() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources = SourcesFile::default();
        save_sources_with(&sources, &fs, &paths).unwrap();
        assert!(fs.file_exists(&paths.sources_path()));
    }

    #[test]
    fn load_and_save_sources_round_trip() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let mut sources = SourcesFile::default();
        sources.sources.insert(
            "test-source".to_string(),
            SourceEntry {
                source_type: SourceType::Local,
                path: Some(PathBuf::from("/some/path")),
                url: None,
                local_clone: None,
                branch: None,
                last_updated: None,
            },
        );

        save_sources_with(&sources, &fs, &paths).unwrap();
        let loaded = load_sources_with(&fs, &paths).unwrap();
        assert_eq!(loaded.sources.len(), 1);
        assert!(loaded.sources.contains_key("test-source"));
    }

    // --- installed_names_with ---

    #[test]
    fn installed_names_returns_empty_when_dir_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let names = installed_names_with(ArtifactKind::Agent, false, &fs, &paths).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn installed_names_filters_md_files_for_agents() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, false);
        fs.add_file(agent_dir.join("alpha.md"), "# agent");
        fs.add_file(agent_dir.join("beta.md"), "# agent");
        fs.add_file(agent_dir.join("not-an-agent.txt"), "ignored");

        let names = installed_names_with(ArtifactKind::Agent, false, &fs, &paths).unwrap();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn installed_names_returns_dirs_for_skills() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let skill_dir = paths.install_dir(ArtifactKind::Skill, false);
        // Skills are directories — add a file inside each to register the dir
        fs.add_file(skill_dir.join("my-skill").join("SKILL.md"), "---\n---\n");
        fs.add_file(skill_dir.join("other-skill").join("SKILL.md"), "---\n---\n");

        let names = installed_names_with(ArtifactKind::Skill, false, &fs, &paths).unwrap();
        assert_eq!(names, vec!["my-skill", "other-skill"]);
    }

    #[test]
    fn installed_names_skips_hidden_entries() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, false);
        fs.add_file(agent_dir.join("visible.md"), "# agent");
        fs.add_file(agent_dir.join(".hidden.md"), "# hidden");

        let names = installed_names_with(ArtifactKind::Agent, false, &fs, &paths).unwrap();
        assert_eq!(names, vec!["visible"]);
    }

    // --- load_config_with / save_config_with ---

    #[test]
    fn load_config_returns_default_when_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let cfg = load_config_with(&fs, &paths).unwrap();
        assert_eq!(cfg.version, 1);
    }

    #[test]
    fn load_and_save_config_round_trip() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let mut cfg = CmxConfig::default();
        cfg.llm.model = "test-model".to_string();
        save_config_with(&cfg, &fs, &paths).unwrap();
        let loaded = load_config_with(&fs, &paths).unwrap();
        assert_eq!(loaded.llm.model, "test-model");
    }

    // Keep the original integration-style tests that use real FS (via the old
    // free functions that now delegate through RealFilesystem).  These are
    // retained from the previously-empty test module; there are no real-FS
    // tests here to preserve since config.rs had no #[cfg(test)] block before.
}
