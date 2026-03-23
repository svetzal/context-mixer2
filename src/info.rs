use anyhow::{Result, bail};
use std::path::Path;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::ArtifactKind;

pub fn info_with(name: &str, ctx: &AppContext<'_>) -> Result<()> {
    // Search both scopes and both kinds
    for local in [false, true] {
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            let dir = ctx.paths.install_dir(kind, local);
            let path = kind.installed_path(name, &dir);
            if ctx.fs.exists(&path) {
                return show_info_with(name, kind, local, &path, ctx);
            }
        }
    }

    bail!("No installed artifact named '{name}' found.");
}

fn show_info_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    path: &Path,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let scope = if local { "local" } else { "global" };
    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let lock_entry = lock.packages.get(name);

    println!("Name:        {name}");
    println!("Type:        {kind}");
    println!("Scope:       {scope}");
    println!("Path:        {}", path.display());

    if let Some(entry) = lock_entry {
        if let Some(v) = &entry.version {
            println!("Version:     {v}");
        }
        println!("Installed:   {}", entry.installed_at);
        println!("Source:      {} ({})", entry.source.repo, entry.source.path);
        println!("Source SHA:  {}", entry.source_checksum);
        println!("Install SHA: {}", entry.installed_checksum);

        // Check for local modifications
        let current_checksum = checksum::checksum_artifact_with(path, kind, ctx.fs)?;
        if current_checksum != entry.installed_checksum {
            println!("Disk SHA:    {current_checksum}  (locally modified)");
        }
    } else {
        println!("Lock entry:  (none — untracked)");
    }

    // Check source for deprecation and available version
    source::auto_update_all_with(ctx).ok();
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        if sa.artifact.name == name && sa.artifact.kind == kind {
            if let Some(dep) = &sa.artifact.deprecation {
                println!("Status:      DEPRECATED");
                if let Some(reason) = &dep.reason {
                    println!("  Reason:    {reason}");
                }
                if let Some(repl) = &dep.replacement {
                    println!("  Replace:   {repl}");
                }
            }
            if let Some(v) = sa.artifact.version.as_deref() {
                let installed_v = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-");
                if v != installed_v {
                    println!("Available:   v{v} (update available)");
                }
            }
        }
    }

    // For skills: list files
    if kind == ArtifactKind::Skill && ctx.fs.is_dir(path) {
        println!();
        println!("Files:");
        list_skill_files_with(path, "  ", ctx)?;
    }

    Ok(())
}

fn list_skill_files_with(dir: &Path, indent: &str, ctx: &AppContext<'_>) -> Result<()> {
    let mut entries = ctx.fs.read_dir(dir)?;
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    for entry in entries {
        let name_str = &entry.file_name;
        if name_str.starts_with('.') {
            continue;
        }

        if entry.is_dir {
            println!("{indent}{name_str}/");
            list_skill_files_with(&entry.path, &format!("{indent}  "), ctx)?;
        } else {
            println!("{indent}{name_str}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::lockfile;
    use crate::paths::ConfigPaths;
    use crate::test_support::{make_ctx, test_paths};
    use crate::types::{
        ArtifactKind, LockEntry, LockFile, LockSource, SourceEntry, SourceType, SourcesFile,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn agent_content(name: &str, desc: &str) -> String {
        format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\n")
    }

    fn setup_source_with_agent(
        fs: &FakeFilesystem,
        paths: &ConfigPaths,
        source_name: &str,
        source_path: &str,
        agent_name: &str,
    ) {
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
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_file(
            format!("{source_path}/{agent_name}.md"),
            agent_content(agent_name, "A test agent"),
        );
    }

    fn install_agent_on_disk(
        fs: &FakeFilesystem,
        paths: &ConfigPaths,
        name: &str,
        content: &str,
        local: bool,
    ) {
        let dir = paths.install_dir(ArtifactKind::Agent, local);
        let path = ArtifactKind::Agent.installed_path(name, &dir);
        fs.add_file(path, content);
    }

    fn write_lock_entry(
        fs: &FakeFilesystem,
        paths: &ConfigPaths,
        name: &str,
        kind: ArtifactKind,
        local: bool,
        source_checksum: &str,
    ) {
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            name.to_string(),
            LockEntry {
                artifact_type: kind,
                version: Some("1.0.0".to_string()),
                installed_at: Utc::now().to_rfc3339(),
                source: LockSource {
                    repo: "my-source".to_string(),
                    path: format!("{name}.md"),
                },
                source_checksum: source_checksum.to_string(),
                installed_checksum: source_checksum.to_string(),
            },
        );
        lockfile::save_with(&lock, local, fs, paths).unwrap();
    }

    // --- info_with ---

    #[test]
    fn info_finds_global_agent() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_agent_on_disk(&fs, &paths, "my-agent", &agent_content("my-agent", "test"), false);

        // Provide an empty sources.json so config::load_sources_with succeeds
        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = info_with("my-agent", &ctx);

        assert!(result.is_ok(), "expected Ok for global agent: {:?}", result.err());
    }

    #[test]
    fn info_finds_local_agent() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_agent_on_disk(&fs, &paths, "my-agent", &agent_content("my-agent", "test"), true);

        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = info_with("my-agent", &ctx);

        assert!(result.is_ok(), "expected Ok for local agent: {:?}", result.err());
    }

    #[test]
    fn info_errors_when_not_found() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = info_with("nonexistent-agent", &ctx);

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found") || msg.contains("nonexistent-agent"),
            "unexpected: {msg}"
        );
    }

    // --- show_info_with ---

    #[test]
    fn show_info_with_tracked_agent_succeeds() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        // Write a lock entry with a checksum that matches the installed content
        write_lock_entry(&fs, &paths, "my-agent", ArtifactKind::Agent, false, "sha256:somecheck");

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let install_dir = paths.install_dir(ArtifactKind::Agent, false);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);
        let result = show_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx);

        assert!(result.is_ok(), "expected Ok for tracked agent: {:?}", result.err());
    }

    #[test]
    fn show_info_untracked_agent_succeeds() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        // No lock entry — untracked
        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let install_dir = paths.install_dir(ArtifactKind::Agent, false);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);
        let result = show_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx);

        assert!(result.is_ok(), "expected Ok for untracked agent: {:?}", result.err());
    }

    // --- list_skill_files_with ---

    #[test]
    fn list_skill_files_succeeds_with_nested_structure() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        fs.add_file("/skills/my-skill/lib/helper.sh", "#!/bin/bash");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = list_skill_files_with(std::path::Path::new("/skills/my-skill"), "  ", &ctx);

        assert!(result.is_ok(), "expected Ok for nested skill: {:?}", result.err());
    }

    #[test]
    fn list_skill_files_empty_dir_succeeds() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_dir("/skills/empty-skill");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = list_skill_files_with(std::path::Path::new("/skills/empty-skill"), "  ", &ctx);

        assert!(result.is_ok(), "expected Ok for empty skill dir: {:?}", result.err());
    }
}
