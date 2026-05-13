use anyhow::{Context, Result};
use std::collections::BTreeMap;
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

/// Load, mutate via `f`, and save the `SourcesFile` in one step.
///
/// `f` is called with a mutable reference to the in-memory sources and may
/// return `Err` to abort without writing; on success the file is saved and
/// the value returned by `f` is propagated.
pub fn mutate_sources_with<F, T>(fs: &dyn Filesystem, paths: &ConfigPaths, f: F) -> Result<T>
where
    F: FnOnce(&mut SourcesFile) -> Result<T>,
{
    let mut sources = load_sources_with(fs, paths)?;
    let result = f(&mut sources)?;
    save_sources_with(&sources, fs, paths)?;
    Ok(result)
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

/// Look up a single installed artifact by name in the lock file.
///
/// Returns `Some(InstalledArtifact)` if the artifact is present in `lock`,
/// or `None` if there is no lock entry for `name`.  `kind` is used to
/// populate the `InstalledArtifact` fields consistently with
/// [`installed_with_lock_data`].
pub fn installed_single_with_lock_data<'a>(
    name: &str,
    lock: &'a LockFile,
    _kind: ArtifactKind,
) -> Option<InstalledArtifact<'a>> {
    let lock_entry = lock.packages.get(name)?;
    Some(InstalledArtifact {
        name: name.to_string(),
        lock_entry: Some(lock_entry),
        installed_version: lock_entry.version.clone(),
    })
}

/// Each element returned by [`match_installed_to_sources`]: an installed
/// artifact paired with its optional source entries.
pub type InstalledWithSources<'a, S> = (InstalledArtifact<'a>, Option<&'a Vec<S>>);

/// Pair every installed artifact of the given `kind` and `local` scope with its
/// source entries from `source_map`, if any.
///
/// Calls [`installed_with_lock_data`] internally and enriches each result with
/// a reference to the matching `Vec<S>` in `source_map` (keyed by artifact
/// name), returning `None` for the source slot when no entry exists.
pub fn match_installed_to_sources<'a, S>(
    kind: ArtifactKind,
    local: bool,
    lock: &'a LockFile,
    source_map: &'a BTreeMap<String, Vec<S>>,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<InstalledWithSources<'a, S>>> {
    let installed = installed_with_lock_data(kind, local, lock, fs, paths)?;
    Ok(installed
        .into_iter()
        .map(|ia| {
            let sources = source_map.get(&ia.name);
            (ia, sources)
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

pub fn resolve_local_path(entry: &SourceEntry) -> Result<PathBuf> {
    match entry.source_type {
        SourceType::Local => entry
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Local source has no path configured")),
        SourceType::Git => entry
            .local_clone
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Git source has no local clone path configured")),
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

    // --- mutate_sources_with ---

    #[test]
    fn mutate_sources_with_loads_applies_and_saves() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        mutate_sources_with(&fs, &paths, |sources| {
            sources
                .sources
                .insert("test-source".to_string(), make_local_entry("/path", None));
            Ok(())
        })
        .unwrap();

        let loaded = load_sources_with(&fs, &paths).unwrap();
        assert!(loaded.sources.contains_key("test-source"));
    }

    #[test]
    fn mutate_sources_with_does_not_save_on_closure_error() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let result: Result<()> =
            mutate_sources_with(&fs, &paths, |_sources| Err(anyhow::anyhow!("closure error")));
        assert!(result.is_err());

        let loaded = load_sources_with(&fs, &paths).unwrap();
        assert!(loaded.sources.is_empty(), "sources should not be saved after closure error");
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

    // --- installed_single_with_lock_data ---

    #[test]
    fn installed_single_with_lock_data_returns_entry_when_present() {
        let paths = test_paths();
        let _ = paths; // paths not needed for this pure function
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

        let result = installed_single_with_lock_data("my-agent", &lock, ArtifactKind::Agent);
        assert!(result.is_some(), "expected Some for present artifact");
        let ia = result.unwrap();
        assert_eq!(ia.name, "my-agent");
        assert_eq!(ia.installed_version.as_deref(), Some("1.0.0"));
        assert!(ia.lock_entry.is_some());
    }

    #[test]
    fn installed_single_with_lock_data_returns_none_when_absent() {
        let lock = LockFile::default();
        let result = installed_single_with_lock_data("missing-agent", &lock, ArtifactKind::Agent);
        assert!(result.is_none(), "expected None for absent artifact");
    }

    // --- match_installed_to_sources ---

    #[test]
    fn match_installed_to_sources_pairs_artifact_with_matching_sources() {
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

        let mut source_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        source_map.insert("my-agent".to_string(), vec!["source-entry".to_string()]);

        let pairs =
            match_installed_to_sources(ArtifactKind::Agent, false, &lock, &source_map, &fs, &paths)
                .unwrap();

        assert_eq!(pairs.len(), 1);
        let (ia, sources) = &pairs[0];
        assert_eq!(ia.name, "my-agent");
        assert_eq!(ia.installed_version.as_deref(), Some("1.0.0"));
        assert!(sources.is_some(), "expected source entries to be found");
        assert_eq!(sources.unwrap(), &["source-entry"]);
    }

    #[test]
    fn match_installed_to_sources_returns_none_when_no_matching_source() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, false);
        fs.add_file(agent_dir.join("orphan-agent.md"), "# agent");

        let lock = LockFile::default();
        let source_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

        let pairs =
            match_installed_to_sources(ArtifactKind::Agent, false, &lock, &source_map, &fs, &paths)
                .unwrap();

        assert_eq!(pairs.len(), 1);
        let (ia, sources) = &pairs[0];
        assert_eq!(ia.name, "orphan-agent");
        assert!(sources.is_none(), "expected no source entries for orphan artifact");
    }

    // --- failure-path tests ---

    #[test]
    fn save_sources_returns_error_when_filesystem_write_fails() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources_path = paths.sources_path();

        // save_json writes to a sibling .tmp file first — fail that write
        fs.set_fail_on_write(crate::json_file::tmp_path(&sources_path));

        let sources = SourcesFile::default();
        let result = save_sources_with(&sources, &fs, &paths);
        assert!(result.is_err(), "expected Err when sources file write fails");

        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to write"), "expected 'Failed to write' in error: {msg}");
    }

    // --- resolve_local_path ---

    #[test]
    fn resolve_local_path_errors_for_local_entry_with_no_path() {
        use crate::types::{SourceEntry, SourceType};
        let entry = SourceEntry {
            source_type: SourceType::Local,
            path: None,
            url: None,
            local_clone: None,
            branch: None,
            last_updated: None,
        };
        let result = resolve_local_path(&entry);
        assert!(result.is_err(), "expected Err when Local source has no path");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no path configured"),
            "expected 'no path configured' in error: {msg}"
        );
    }

    #[test]
    fn resolve_local_path_errors_for_git_entry_with_no_local_clone() {
        use crate::types::{SourceEntry, SourceType};
        let entry = SourceEntry {
            source_type: SourceType::Git,
            path: None,
            url: Some("https://github.com/example/repo.git".to_string()),
            local_clone: None,
            branch: None,
            last_updated: None,
        };
        let result = resolve_local_path(&entry);
        assert!(result.is_err(), "expected Err when Git source has no local clone");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no local clone"), "expected 'no local clone' in error: {msg}");
    }

    #[test]
    fn resolve_local_path_returns_path_for_local_entry() {
        let entry = make_local_entry("/some/path", None);
        let result = resolve_local_path(&entry);
        assert!(result.is_ok(), "expected Ok for local entry with path");
        assert_eq!(result.unwrap(), std::path::PathBuf::from("/some/path"));
    }
}
