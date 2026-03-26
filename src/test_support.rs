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

#[cfg(all(test, feature = "llm"))]
pub(crate) fn install_skill_on_disk(
    fs: &crate::gateway::fakes::FakeFilesystem,
    paths: &crate::paths::ConfigPaths,
    name: &str,
    files: &[(&str, &str)],
    local: bool,
) {
    let dir = paths.install_dir(crate::types::ArtifactKind::Skill, local);
    let skill_dir = dir.join(name);
    for (file_name, content) in files {
        fs.add_file(skill_dir.join(file_name), *content);
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
    use crate::types::{SourceEntry, SourceType, SourcesFile};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    let sources = SourcesFile {
        version: 1,
        sources: {
            let mut m = BTreeMap::new();
            m.insert(
                source_name.to_string(),
                SourceEntry {
                    source_type: SourceType::Local,
                    path: Some(PathBuf::from(source_path)),
                    url: None,
                    local_clone: None,
                    branch: None,
                    last_updated: Some(Utc::now().to_rfc3339()),
                },
            );
            m
        },
    };
    let sources_json = serde_json::to_string_pretty(&sources).unwrap();
    fs.add_file(paths.sources_path(), sources_json);

    fs.add_file(
        format!("{source_path}/{agent_name}.md"),
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
