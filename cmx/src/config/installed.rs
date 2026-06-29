use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::gateway::filesystem::Filesystem;
use crate::paths::ConfigPaths;
use crate::types::{ArtifactKind, InstallScope, InstalledArtifact, LockFile};

/// Whether an artifact at `location` named `name` matches one of the `external`
/// rules (artifacts another tool manages — see [`CmxConfig::external`]).
///
/// Each rule is either a **directory** (contains a path separator or starts with
/// `~` — matches when `location` is at or below it) or a bare **name** (matches
/// when it equals `name`). `home_dir` expands a leading `~` in directory rules.
///
/// [`CmxConfig::external`]: crate::types::CmxConfig::external
pub fn matches_external(
    external: &[String],
    name: &str,
    location: &std::path::Path,
    home_dir: &std::path::Path,
) -> bool {
    external.iter().any(|rule| {
        if rule.contains('/') || rule.starts_with('~') {
            location.starts_with(super::expand_tilde(rule, home_dir))
        } else {
            rule == name
        }
    })
}

pub fn installed_names(
    kind: ArtifactKind,
    scope: InstallScope,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<String>> {
    let Some(dir) = paths.install_dir(kind, scope) else {
        return Ok(Vec::new());
    };
    if !fs.exists(&dir) {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in fs.read_dir(&dir)? {
        if entry.file_name.starts_with('.') {
            continue;
        }

        if let Some(name) = kind.artifact_name_from_entry(&entry, paths.platform.agent_extension())
        {
            names.push(name);
        }
    }

    names.sort();
    Ok(names)
}

pub fn installed_with_lock_data<'a>(
    kind: ArtifactKind,
    scope: InstallScope,
    lock: &'a LockFile,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<InstalledArtifact<'a>>> {
    let names = installed_names(kind, scope, fs, paths)?;
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
    scope: InstallScope,
    lock: &'a LockFile,
    source_map: &'a BTreeMap<String, Vec<S>>,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<InstalledWithSources<'a, S>>> {
    let installed = installed_with_lock_data(kind, scope, lock, fs, paths)?;
    Ok(installed
        .into_iter()
        .map(|ia| {
            let sources = source_map.get(&ia.name);
            (ia, sources)
        })
        .collect())
}

/// Search for an installed artifact on disk, checking global scope first then
/// local.  Returns the path and scope it was found in, or `None` if not found
/// in either scope.
pub fn find_installed_path(
    name: &str,
    kind: ArtifactKind,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Option<(PathBuf, InstallScope)> {
    for scope in InstallScope::ALL {
        let Some(path) = paths.installed_artifact_path(kind, name, scope) else {
            continue;
        };
        if fs.exists(&path) {
            return Some((path, scope));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{
        add_skill, install_skill_on_disk, make_lock_entry_versioned, skill_content, test_paths,
    };

    // --- installed_names_with ---

    #[test]
    fn installed_names_returns_empty_when_dir_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let names =
            installed_names(ArtifactKind::Agent, InstallScope::Global, &fs, &paths).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn installed_names_filters_md_files_for_agents() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        fs.add_file(agent_dir.join("alpha.md"), "# agent");
        fs.add_file(agent_dir.join("beta.md"), "# agent");
        fs.add_file(agent_dir.join("not-an-agent.txt"), "ignored");

        let names =
            installed_names(ArtifactKind::Agent, InstallScope::Global, &fs, &paths).unwrap();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn installed_names_returns_dirs_for_skills() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let skill_dir = paths.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
        // Skills are directories — add a file inside each to register the dir
        add_skill(&fs, &skill_dir, "my-skill", "");
        add_skill(&fs, &skill_dir, "other-skill", "");

        let names =
            installed_names(ArtifactKind::Skill, InstallScope::Global, &fs, &paths).unwrap();
        assert_eq!(names, vec!["my-skill", "other-skill"]);
    }

    #[test]
    fn installed_names_skips_hidden_entries() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        fs.add_file(agent_dir.join("visible.md"), "# agent");
        fs.add_file(agent_dir.join(".hidden.md"), "# hidden");

        let names =
            installed_names(ArtifactKind::Agent, InstallScope::Global, &fs, &paths).unwrap();
        assert_eq!(names, vec!["visible"]);
    }

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
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let result = find_installed_path("my-agent", ArtifactKind::Agent, &fs, &paths);
        assert!(result.is_some(), "expected Some for installed global agent");
        let (_, scope) = result.unwrap();
        assert_eq!(scope, InstallScope::Global, "expected global scope");
    }

    #[test]
    fn find_installed_path_finds_local_agent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Local).unwrap();
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let result = find_installed_path("my-agent", ArtifactKind::Agent, &fs, &paths);
        assert!(result.is_some(), "expected Some for installed local agent");
        let (_, scope) = result.unwrap();
        assert_eq!(scope, InstallScope::Local, "expected local scope");
    }

    #[test]
    fn find_installed_path_prefers_global_over_local() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        // Install in both scopes
        fs.add_file(
            paths
                .install_dir(ArtifactKind::Agent, InstallScope::Global)
                .unwrap()
                .join("my-agent.md"),
            "global",
        );
        fs.add_file(
            paths
                .install_dir(ArtifactKind::Agent, InstallScope::Local)
                .unwrap()
                .join("my-agent.md"),
            "local",
        );

        let result = find_installed_path("my-agent", ArtifactKind::Agent, &fs, &paths);
        let (_, scope) = result.unwrap();
        assert_eq!(scope, InstallScope::Global, "expected global to be preferred over local");
    }

    #[test]
    fn find_installed_path_finds_skill_directory() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        install_skill_on_disk(&fs, &paths, "my-skill", &skill_content(""), InstallScope::Global);

        let result = find_installed_path("my-skill", ArtifactKind::Skill, &fs, &paths);
        assert!(result.is_some(), "expected Some for installed global skill");
        let (_, scope) = result.unwrap();
        assert_eq!(scope, InstallScope::Global, "expected global scope");
    }

    // --- installed_with_lock_data ---

    #[test]
    fn installed_with_lock_data_returns_name_and_lock_entry() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
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
            installed_with_lock_data(ArtifactKind::Agent, InstallScope::Global, &lock, &fs, &paths)
                .unwrap();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "my-agent");
        assert!(artifacts[0].lock_entry.is_some());
        assert_eq!(artifacts[0].installed_version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn installed_with_lock_data_absent_lock_entry_gives_none_version() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        fs.add_file(agent_dir.join("my-agent.md"), "# agent");

        let lock = LockFile::default();

        let artifacts =
            installed_with_lock_data(ArtifactKind::Agent, InstallScope::Global, &lock, &fs, &paths)
                .unwrap();
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
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
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

        let pairs = match_installed_to_sources(
            ArtifactKind::Agent,
            InstallScope::Global,
            &lock,
            &source_map,
            &fs,
            &paths,
        )
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
        let agent_dir = paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        fs.add_file(agent_dir.join("orphan-agent.md"), "# agent");

        let lock = LockFile::default();
        let source_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

        let pairs = match_installed_to_sources(
            ArtifactKind::Agent,
            InstallScope::Global,
            &lock,
            &source_map,
            &fs,
            &paths,
        )
        .unwrap();

        assert_eq!(pairs.len(), 1);
        let (ia, sources) = &pairs[0];
        assert_eq!(ia.name, "orphan-agent");
        assert!(sources.is_none(), "expected no source entries for orphan artifact");
    }

    // --- matches_external ---

    #[test]
    fn matches_external_by_directory_with_tilde() {
        let home = std::path::Path::new("/home/u");
        let rules = vec!["~/.hermes/skills".to_string()];
        // An artifact under the declared dir matches; one elsewhere does not.
        assert!(matches_external(
            &rules,
            "apple",
            std::path::Path::new("/home/u/.hermes/skills"),
            home
        ));
        assert!(!matches_external(
            &rules,
            "mine",
            std::path::Path::new("/home/u/.claude/skills"),
            home
        ));
    }

    #[test]
    fn matches_external_by_bare_name() {
        let home = std::path::Path::new("/home/u");
        let rules = vec!["apple".to_string()];
        // Name rule matches regardless of location.
        assert!(matches_external(&rules, "apple", std::path::Path::new("/anywhere"), home));
        assert!(!matches_external(&rules, "banana", std::path::Path::new("/anywhere"), home));
    }

    #[test]
    fn matches_external_empty_rules_never_match() {
        let home = std::path::Path::new("/home/u");
        assert!(!matches_external(
            &[],
            "apple",
            std::path::Path::new("/home/u/.hermes/skills"),
            home
        ));
    }
}
