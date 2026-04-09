#[cfg(test)]
pub(crate) fn agent_content(name: &str, desc: &str) -> String {
    format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n")
}

#[cfg(test)]
pub(crate) fn skill_content(desc: &str) -> String {
    format!("---\ndescription: {desc}\n---\n# skill\n")
}

#[cfg(test)]
pub(crate) fn versioned_skill_content(desc: &str, version: &str) -> String {
    format!("---\ndescription: {desc}\nversion: {version}\n---\n# skill\n")
}

#[cfg(test)]
pub(crate) fn versioned_agent_content(name: &str, desc: &str, version: &str) -> String {
    format!("---\nname: {name}\ndescription: {desc}\nversion: {version}\n---\n# {name}\n")
}

#[cfg(test)]
pub(crate) fn metadata_versioned_agent_content(name: &str, desc: &str, version: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {desc}\nmetadata:\n  version: \"{version}\"\n  author: Test\n---\n# {name}\n"
    )
}

#[cfg(test)]
pub(crate) fn metadata_versioned_skill_content(desc: &str, version: &str) -> String {
    format!(
        "---\ndescription: {desc}\nmetadata:\n  version: \"{version}\"\n  author: Test\n---\n# skill\n"
    )
}

#[cfg(test)]
pub(crate) fn deprecated_agent_content(
    name: &str,
    desc: &str,
    reason: &str,
    replacement: &str,
) -> String {
    format!(
        "---\nname: {name}\ndescription: {desc}\ndeprecated: true\ndeprecated_reason: {reason}\ndeprecated_replacement: {replacement}\n---\n"
    )
}

#[cfg(test)]
pub(crate) fn make_local_entry(
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

#[cfg(test)]
pub(crate) fn make_git_entry(
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

#[cfg(test)]
pub(crate) fn make_lock_entry_builder(
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

#[cfg(test)]
pub(crate) fn make_lock_entry_versioned(
    kind: crate::types::ArtifactKind,
    version: &str,
    repo: &str,
    path: &str,
) -> crate::types::LockEntry {
    let mut entry = make_lock_entry_builder(kind, repo, path);
    entry.version = Some(version.to_string());
    entry
}

#[cfg(test)]
pub(crate) fn make_lock_entry_with_checksum(
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

#[cfg(test)]
pub(crate) fn save_lock_with_entry(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    name: &str,
    entry: crate::types::LockEntry,
    local: bool,
) {
    use crate::types::LockFile;
    use std::collections::BTreeMap;
    let mut packages = BTreeMap::new();
    packages.insert(name.to_string(), entry);
    let lock = LockFile {
        version: 1,
        packages,
    };
    crate::lockfile::save_with(&lock, local, fs, paths).unwrap();
}

#[cfg(test)]
pub(crate) fn setup_source(
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

#[cfg(test)]
pub(crate) fn setup_sources(
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

#[cfg(test)]
pub(crate) fn setup_source_with_versioned_agent(
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

#[cfg(test)]
pub(crate) fn setup_source_with_skill(
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

#[cfg(test)]
pub(crate) fn sample_lock_entry() -> crate::types::LockEntry {
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

#[cfg(test)]
pub(crate) fn sample_lock_file() -> crate::types::LockFile {
    use crate::types::LockFile;
    use std::collections::BTreeMap;
    let mut packages = BTreeMap::new();
    packages.insert("my-agent".to_string(), sample_lock_entry());
    LockFile {
        version: 1,
        packages,
    }
}

#[cfg(test)]
pub(crate) fn install_agent_on_disk(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    name: &str,
    content: &str,
    local: bool,
) {
    let dir = paths.install_dir(crate::types::ArtifactKind::Agent, local);
    let path = crate::types::ArtifactKind::Agent.installed_path(name, &dir);
    fs.add_file(path, content);
}

#[cfg(test)]
pub(crate) fn setup_source_with_agent(
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

#[cfg(test)]
pub(crate) fn test_paths() -> crate::paths::ConfigPaths {
    use std::path::PathBuf;
    crate::paths::ConfigPaths::for_test(
        PathBuf::from("/home/testuser"),
        PathBuf::from("/home/testuser/.config/context-mixer"),
    )
}

#[cfg(test)]
pub(crate) fn make_ctx<'a>(
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
