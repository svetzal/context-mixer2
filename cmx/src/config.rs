use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::gateway::filesystem::Filesystem;
use crate::paths::ConfigPaths;
use crate::types::{
    ArtifactKind, CmxConfig, InstalledArtifact, LockFile, SourceEntry, SourceType, SourcesFile,
};

// ---------------------------------------------------------------------------
// Testable variants (accept injected Filesystem + ConfigPaths)
// ---------------------------------------------------------------------------

pub fn load_sources_with(fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<SourcesFile> {
    crate::json_file::load_json(&paths.sources_path(), fs)
}

pub fn save_sources_with(
    sources: &SourcesFile,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<()> {
    crate::json_file::save_json(sources, &paths.sources_path(), fs)
}

pub fn load_config_with(fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<CmxConfig> {
    crate::json_file::load_json(&paths.config_path(), fs)
}

pub fn save_config_with(
    config: &CmxConfig,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<()> {
    crate::json_file::save_json(config, &paths.config_path(), fs)
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

        if let Some(name) = kind.artifact_name_from_entry(&entry) {
            names.push(name);
        }
    }

    names.sort();
    Ok(names)
}

pub fn installed_with_lock_data<'a>(
    kind: ArtifactKind,
    local: bool,
    lock: &'a LockFile,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<InstalledArtifact<'a>>> {
    let names = installed_names_with(kind, local, fs, paths)?;
    Ok(names
        .into_iter()
        .map(|name| {
            let lock_entry = lock.packages.get(&name);
            let installed_version = lock_entry.and_then(|e| e.version.clone());
            InstalledArtifact {
                name,
                lock_entry,
                installed_version,
            }
        })
        .collect())
}

/// Search for an installed artifact on disk, checking global scope first then
/// local.  Returns the path and whether the artifact is in local scope, or
/// `None` if not found in either scope.
pub fn find_installed_path(
    name: &str,
    kind: ArtifactKind,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Option<(PathBuf, bool)> {
    for local in [false, true] {
        let dir = paths.install_dir(kind, local);
        let path = kind.installed_path(name, &dir);
        if fs.exists(&path) {
            return Some((path, local));
        }
    }
    None
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
    use crate::test_support::{make_local_entry, make_lock_entry_versioned, test_paths};
    use crate::types::LockFile;

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
        sources
            .sources
            .insert("test-source".to_string(), make_local_entry("/some/path", None));

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

    // --- find_installed_path ---

    #[test]
    fn find_installed_path_returns_none_when_not_installed() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let result = find_installed_path("nonexistent", ArtifactKind::Agent, &fs, &paths);
        assert!(result.is_none(), "expected None when artifact is not installed");
    }

    #[test]
    fn find_installed_path_finds_global_agent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, false);
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let result = find_installed_path("my-agent", ArtifactKind::Agent, &fs, &paths);
        assert!(result.is_some(), "expected Some for installed global agent");
        let (_, local) = result.unwrap();
        assert!(!local, "expected global scope (local=false)");
    }

    #[test]
    fn find_installed_path_finds_local_agent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, true);
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let result = find_installed_path("my-agent", ArtifactKind::Agent, &fs, &paths);
        assert!(result.is_some(), "expected Some for installed local agent");
        let (_, local) = result.unwrap();
        assert!(local, "expected local scope (local=true)");
    }

    #[test]
    fn find_installed_path_prefers_global_over_local() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        // Install in both scopes
        fs.add_file(paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md"), "global");
        fs.add_file(paths.install_dir(ArtifactKind::Agent, true).join("my-agent.md"), "local");

        let result = find_installed_path("my-agent", ArtifactKind::Agent, &fs, &paths);
        let (_, local) = result.unwrap();
        assert!(!local, "expected global to be preferred over local");
    }

    #[test]
    fn find_installed_path_finds_skill_directory() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let skill_dir = paths.install_dir(ArtifactKind::Skill, false).join("my-skill");
        fs.add_file(skill_dir.join("SKILL.md"), "---\n---\n");

        let result = find_installed_path("my-skill", ArtifactKind::Skill, &fs, &paths);
        assert!(result.is_some(), "expected Some for installed global skill");
        let (_, local) = result.unwrap();
        assert!(!local, "expected global scope (local=false)");
    }

    // --- installed_with_lock_data ---

    #[test]
    fn installed_with_lock_data_returns_name_and_lock_entry() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, false);
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let mut lock = LockFile::default();
        lock.packages.insert(
            "my-agent".to_string(),
            make_lock_entry_versioned(
                ArtifactKind::Agent,
                "1.0.0",
                "guidelines",
                "agents/my-agent.md",
            ),
        );

        let artifacts =
            installed_with_lock_data(ArtifactKind::Agent, false, &lock, &fs, &paths).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "my-agent");
        assert!(artifacts[0].lock_entry.is_some());
        assert_eq!(artifacts[0].installed_version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn installed_with_lock_data_absent_lock_entry_gives_none_version() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, false);
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let lock = LockFile::default();

        let artifacts =
            installed_with_lock_data(ArtifactKind::Agent, false, &lock, &fs, &paths).unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "my-agent");
        assert!(artifacts[0].lock_entry.is_none());
        assert!(artifacts[0].installed_version.is_none());
    }

    // --- failure-path tests ---

    #[test]
    fn save_sources_returns_error_when_filesystem_write_fails() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources_path = paths.sources_path();

        // Cause the write to the sources file to fail
        fs.set_fail_on_write(sources_path);

        let sources = SourcesFile::default();
        let result = save_sources_with(&sources, &fs, &paths);
        assert!(result.is_err(), "expected Err when sources file write fails");

        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to write"), "expected 'Failed to write' in error: {msg}");
    }
}
