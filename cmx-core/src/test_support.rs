//! Test helpers shared across unit and integration tests.
//!
//! Available in two contexts:
//! - Within `cmx-core` itself: via `#[cfg(test)]`
//! - In downstream crates (e.g. `cmx`): via the `test-support` feature flag

/// Build minimal agent markdown content with a `name`/`description` frontmatter.
pub fn agent_content(name: &str, desc: &str) -> String {
    format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n")
}

/// Build minimal `SKILL.md` content with a `description` frontmatter.
pub fn skill_content(desc: &str) -> String {
    format!("---\ndescription: {desc}\n---\n# skill\n")
}

/// Build `SKILL.md` content with a top-level `version:` key (the legacy,
/// shadowing form frontmatter reconciliation removes).
pub fn versioned_skill_content(desc: &str, version: &str) -> String {
    format!("---\ndescription: {desc}\nversion: {version}\n---\n# skill\n")
}

/// Build agent markdown content with a top-level `version:` key.
pub fn versioned_agent_content(name: &str, desc: &str, version: &str) -> String {
    format!("---\nname: {name}\ndescription: {desc}\nversion: {version}\n---\n# {name}\n")
}

/// Build agent markdown content with the version nested under `metadata.version`
/// (the community-standard, non-shadowing form).
pub fn metadata_versioned_agent_content(name: &str, desc: &str, version: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {desc}\nmetadata:\n  version: \"{version}\"\n  author: Test\n---\n# {name}\n"
    )
}

/// Build `SKILL.md` content with the version nested under `metadata.version`.
pub fn metadata_versioned_skill_content(desc: &str, version: &str) -> String {
    format!(
        "---\ndescription: {desc}\nmetadata:\n  version: \"{version}\"\n  author: Test\n---\n# skill\n"
    )
}

/// Build agent markdown content marked deprecated, with a reason and replacement.
pub fn deprecated_agent_content(name: &str, desc: &str, reason: &str, replacement: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {desc}\ndeprecated: true\ndeprecated_reason: {reason}\ndeprecated_replacement: {replacement}\n---\n"
    )
}

/// Build a [`crate::types::SourceEntry`] for a local-directory source.
pub fn make_local_entry(
    path: impl Into<std::path::PathBuf>,
    last_updated: Option<String>,
) -> crate::types::SourceEntry {
    use crate::types::{SourceEntry, SourceType};
    SourceEntry {
        source_type: SourceType::Local,
        path: Some(path.into()),
        url: None,
        local_clone: None,
        branch: None,
        last_updated,
    }
}

/// Build a [`crate::types::SourceEntry`] for a git-remote source.
pub fn make_git_entry(
    url: &str,
    clone_path: impl Into<std::path::PathBuf>,
    branch: &str,
    last_updated: Option<String>,
) -> crate::types::SourceEntry {
    use crate::types::{SourceEntry, SourceType};
    SourceEntry {
        source_type: SourceType::Git,
        path: None,
        url: Some(url.to_string()),
        local_clone: Some(clone_path.into()),
        branch: Some(branch.to_string()),
        last_updated,
    }
}

/// Build a [`crate::types::LockEntry`] with placeholder checksums and no version.
pub fn make_lock_entry_builder(
    kind: crate::types::ArtifactKind,
    repo: &str,
    path: &str,
) -> crate::types::LockEntry {
    use crate::types::{LockEntry, LockSource};
    LockEntry {
        artifact_type: kind,
        version: None,
        installed_at: "2024-01-01T00:00:00Z".to_string(),
        source: LockSource {
            repo: repo.to_string(),
            path: path.to_string(),
        },
        source_checksum: "sha256:placeholder".to_string(),
        installed_checksum: "sha256:placeholder".to_string(),
    }
}

/// Like [`make_lock_entry_builder`], with an explicit version set.
pub fn make_lock_entry_versioned(
    kind: crate::types::ArtifactKind,
    version: &str,
    repo: &str,
    path: &str,
) -> crate::types::LockEntry {
    let mut entry = make_lock_entry_builder(kind, repo, path);
    entry.version = Some(version.to_string());
    entry
}

/// Like [`make_lock_entry_builder`], with an explicit version and matching
/// source/installed checksum.
pub fn make_lock_entry_with_checksum(
    kind: crate::types::ArtifactKind,
    version: Option<&str>,
    repo: &str,
    path: &str,
    checksum: &str,
) -> crate::types::LockEntry {
    let mut entry = make_lock_entry_builder(kind, repo, path);
    entry.version = version.map(str::to_string);
    entry.source_checksum = checksum.to_string();
    entry.installed_checksum = checksum.to_string();
    entry
}

/// Save a lock file for `scope` containing a single `name` → `entry` package.
pub fn save_lock_with_entry(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    name: &str,
    entry: crate::types::LockEntry,
    scope: crate::types::InstallScope,
) {
    use crate::types::LockFile;
    use std::collections::BTreeMap;
    let mut packages = BTreeMap::new();
    packages.insert(name.to_string(), entry);
    let lock = LockFile {
        version: 1,
        packages,
    };
    crate::lockfile::save(&lock, scope, fs, paths).unwrap();
}

/// Write `sources.json` registering a single local source named `source_name`.
pub fn setup_source(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    source_name: &str,
    source_path: &str,
) {
    use crate::types::SourcesFile;
    use chrono::Utc;
    use std::collections::BTreeMap;

    let sources = SourcesFile {
        version: 1,
        sources: {
            let mut m = BTreeMap::new();
            m.insert(
                source_name.to_string(),
                make_local_entry(source_path, Some(Utc::now().to_rfc3339())),
            );
            m
        },
    };
    let sources_json = serde_json::to_string_pretty(&sources).unwrap();
    fs.add_file(paths.sources_path(), sources_json);
}

/// Write `sources.json` registering multiple local sources from `(name, path)` pairs.
pub fn setup_sources(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    entries: &[(&str, &str)],
) {
    use crate::types::SourcesFile;
    use chrono::Utc;
    use std::collections::BTreeMap;

    let sources = SourcesFile {
        version: 1,
        sources: {
            let mut m = BTreeMap::new();
            for &(name, path) in entries {
                m.insert(name.to_string(), make_local_entry(path, Some(Utc::now().to_rfc3339())));
            }
            m
        },
    };
    let sources_json = serde_json::to_string_pretty(&sources).unwrap();
    fs.add_file(paths.sources_path(), sources_json);
}

/// Register a local source and write one versioned agent file into it.
pub fn setup_source_with_versioned_agent(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    source_name: &str,
    source_path: &str,
    agent_name: &str,
    version: &str,
) {
    setup_source(fs, paths, source_name, source_path);
    fs.add_file(
        format!("{source_path}/agents/{agent_name}.md"),
        versioned_agent_content(agent_name, "A test agent", version),
    );
}

/// Register a local source and write one versioned skill into it.
pub fn setup_source_with_skill(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    source_name: &str,
    source_path: &str,
    skill_name: &str,
    skill_version: &str,
) {
    setup_source(fs, paths, source_name, source_path);
    fs.add_file(
        format!("{source_path}/{skill_name}/SKILL.md"),
        versioned_skill_content("A test skill", skill_version),
    );
}

/// A representative, fully-populated agent [`crate::types::LockEntry`] for tests.
pub fn sample_lock_entry() -> crate::types::LockEntry {
    use crate::types::{ArtifactKind, LockEntry, LockSource};
    LockEntry {
        artifact_type: ArtifactKind::Agent,
        version: Some("1.0.0".to_string()),
        installed_at: "2024-01-01T00:00:00Z".to_string(),
        source: LockSource {
            repo: "guidelines".to_string(),
            path: "agents/my-agent.md".to_string(),
        },
        source_checksum: "sha256:abc123".to_string(),
        installed_checksum: "sha256:def456".to_string(),
    }
}

/// A [`crate::types::LockFile`] containing one [`sample_lock_entry`].
pub fn sample_lock_file() -> crate::types::LockFile {
    use crate::types::LockFile;
    use std::collections::BTreeMap;
    let mut packages = BTreeMap::new();
    packages.insert("my-agent".to_string(), sample_lock_entry());
    LockFile {
        version: 1,
        packages,
    }
}

/// Write `content` to disk at the path an agent named `name` would install to.
pub fn install_agent_on_disk(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    name: &str,
    content: &str,
    scope: crate::types::InstallScope,
) {
    let path = paths
        .installed_artifact_path(crate::types::ArtifactKind::Agent, name, scope)
        .expect("install_agent_on_disk: caller uses platform that supports Agent");
    fs.add_file(path, content);
}

/// Write a `SKILL.md` with `content` at the path a skill named `name` would install to.
pub fn install_skill_on_disk(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    name: &str,
    content: &str,
    scope: crate::types::InstallScope,
) {
    let dir = paths
        .installed_artifact_path(crate::types::ArtifactKind::Skill, name, scope)
        .expect("install_skill_on_disk: caller uses platform that supports Skill");
    fs.add_file(dir.join("SKILL.md"), content);
}

/// Write a `SKILL.md` for `name` directly under `dir` (outside any install-dir
/// resolution — for building an arbitrary source-repo tree in tests).
pub fn add_skill(
    fs: &crate::gateway::fakes::FakeFilesystem,
    dir: impl AsRef<std::path::Path>,
    name: &str,
    desc: &str,
) {
    fs.add_file(dir.as_ref().join(name).join("SKILL.md"), skill_content(desc));
}

/// Register a local source and write one unversioned agent file into it.
pub fn setup_source_with_agent(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    source_name: &str,
    source_path: &str,
    agent_name: &str,
) {
    setup_source(fs, paths, source_name, source_path);
    fs.add_file(
        format!("{source_path}/agents/{agent_name}.md"),
        agent_content(agent_name, "A test agent"),
    );
}

/// A [`crate::paths::ConfigPaths`] rooted at fake `/home/testuser` directories,
/// bound to [`crate::platform::Platform::Claude`].
pub fn test_paths() -> crate::paths::ConfigPaths {
    use std::path::PathBuf;
    crate::paths::ConfigPaths::for_test(
        PathBuf::from("/home/testuser"),
        PathBuf::from("/home/testuser/.config/context-mixer"),
    )
}

/// Like [`test_paths`], bound to an explicit `platform`.
pub fn test_paths_for(platform: crate::platform::Platform) -> crate::paths::ConfigPaths {
    use std::path::PathBuf;
    crate::paths::ConfigPaths::for_test_with_platform(
        PathBuf::from("/home/testuser"),
        PathBuf::from("/home/testuser/.config/context-mixer"),
        platform,
    )
}

/// Write an empty (default) `sources.json`.
pub fn setup_empty_sources(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
) {
    let sources = crate::types::SourcesFile::default();
    fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
}

/// Write `sources.json` registering a single git-remote source.
pub fn setup_source_git(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    source_name: &str,
    url: &str,
    clone_path: impl Into<std::path::PathBuf>,
    branch: &str,
    last_updated: Option<String>,
) {
    let mut sources = crate::types::SourcesFile::default();
    sources
        .sources
        .insert(source_name.to_string(), make_git_entry(url, clone_path, branch, last_updated));
    fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
}

/// Write `sources.json` from a slice of pre-built `(name, entry)` pairs.
pub fn setup_sources_from_entries(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    entries: &[(&str, crate::types::SourceEntry)],
) {
    let mut sources = crate::types::SourcesFile::default();
    for (name, entry) in entries {
        sources.sources.insert(name.to_string(), entry.clone());
    }
    fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
}

/// Build an [`crate::context::AppContext`] from fake gateways and `paths`, with
/// no LLM client.
pub fn make_ctx<'a>(
    fs: &'a crate::gateway::fakes::FakeFilesystem,
    git: &'a crate::gateway::fakes::FakeGitClient,
    clock: &'a crate::gateway::fakes::FakeClock,
    paths: &'a crate::paths::ConfigPaths,
) -> crate::context::AppContext<'a> {
    crate::context::AppContext {
        fs,
        git,
        clock,
        paths,
        llm: None,
    }
}

/// An in-memory context for exercising an embedding tool's integration without
/// touching the real filesystem — bundles fake gateways with test paths, ready to
/// hand to [`TestContext::ctx`].
pub struct TestContext {
    /// The in-memory fake filesystem.
    pub fs: crate::gateway::fakes::FakeFilesystem,
    /// The fake git client (records calls without running git).
    pub git: crate::gateway::fakes::FakeGitClient,
    /// The fake clock, fixed at construction time.
    pub clock: crate::gateway::fakes::FakeClock,
    /// Fake config/install paths rooted under `/home/testuser`.
    pub paths: crate::paths::ConfigPaths,
}

impl TestContext {
    /// Build a `TestContext` bound to [`crate::platform::Platform::Claude`], with
    /// the clock fixed at the current real time.
    pub fn new() -> Self {
        Self::build(test_paths(), crate::gateway::fakes::FakeClock::at(chrono::Utc::now()))
    }

    /// Build a `TestContext` bound to an explicit `platform`.
    pub fn for_platform(platform: crate::platform::Platform) -> Self {
        Self::build(
            test_paths_for(platform),
            crate::gateway::fakes::FakeClock::at(chrono::Utc::now()),
        )
    }

    /// Build a `TestContext` with the clock fixed at an explicit `time`.
    pub fn at(time: chrono::DateTime<chrono::Utc>) -> Self {
        Self::build(test_paths(), crate::gateway::fakes::FakeClock::at(time))
    }

    fn build(paths: crate::paths::ConfigPaths, clock: crate::gateway::fakes::FakeClock) -> Self {
        Self {
            fs: crate::gateway::fakes::FakeFilesystem::new(),
            git: crate::gateway::fakes::FakeGitClient::new(),
            clock,
            paths,
        }
    }

    /// Borrow an [`crate::context::AppContext`] backed by this context's fakes.
    pub fn ctx(&self) -> crate::context::AppContext<'_> {
        make_ctx(&self.fs, &self.git, &self.clock, &self.paths)
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}
