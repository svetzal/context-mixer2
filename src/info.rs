use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::{ArtifactKind, Deprecation};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub(crate) struct ArtifactInfo {
    pub name: String,
    pub kind: ArtifactKind,
    pub scope: &'static str,
    pub path: PathBuf,
    pub version: Option<String>,
    pub installed_at: Option<String>,
    pub source_display: Option<String>,
    pub source_checksum: Option<String>,
    pub installed_checksum: Option<String>,
    pub disk_checksum: Option<String>,
    pub locally_modified: bool,
    pub untracked: bool,
    pub deprecation: Option<Deprecation>,
    pub available_version: Option<String>,
    pub skill_files: Vec<SkillFileEntry>,
}

pub(crate) struct SkillFileEntry {
    pub name: String,
    pub is_dir: bool,
    pub indent_level: usize,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn info_with(name: &str, ctx: &AppContext<'_>) -> Result<()> {
    // Search both scopes and both kinds
    for local in [false, true] {
        for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
            let dir = ctx.paths.install_dir(kind, local);
            let path = kind.installed_path(name, &dir);
            if ctx.fs.exists(&path) {
                let info = gather_info_with(name, kind, local, &path, ctx)?;
                print_info(&info);
                return Ok(());
            }
        }
    }

    bail!("No installed artifact named '{name}' found.");
}

// ---------------------------------------------------------------------------
// Gather (pure logic, no println!)
// ---------------------------------------------------------------------------

pub(crate) fn gather_info_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    path: &Path,
    ctx: &AppContext<'_>,
) -> Result<ArtifactInfo> {
    let scope = if local { "local" } else { "global" };
    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let lock_entry = lock.packages.get(name);

    let (
        version,
        installed_at,
        source_display,
        source_checksum,
        installed_checksum,
        disk_checksum,
        locally_modified,
        untracked,
    ) = if let Some(entry) = lock_entry {
        let current_checksum = checksum::checksum_artifact_with(path, kind, ctx.fs)?;
        let locally_modified = current_checksum != entry.installed_checksum;
        let disk_checksum = if locally_modified {
            Some(current_checksum)
        } else {
            None
        };
        (
            entry.version.clone(),
            Some(entry.installed_at.clone()),
            Some(format!("{} ({})", entry.source.repo, entry.source.path)),
            Some(entry.source_checksum.clone()),
            Some(entry.installed_checksum.clone()),
            disk_checksum,
            locally_modified,
            false,
        )
    } else {
        (None, None, None, None, None, None, false, true)
    };

    // Check source for deprecation and available version
    source::auto_update_all_with(ctx).ok();
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let mut deprecation: Option<Deprecation> = None;
    let mut available_version: Option<String> = None;

    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        if sa.artifact.name == name && sa.artifact.kind == kind {
            if sa.artifact.deprecation.is_some() {
                deprecation = sa.artifact.deprecation;
            }
            if let Some(v) = sa.artifact.version.as_deref() {
                let installed_v = lock_entry.and_then(|e| e.version.as_deref()).unwrap_or("-");
                if v != installed_v {
                    available_version = Some(v.to_string());
                }
            }
        }
    }

    // For skills: collect files
    let skill_files = if kind == ArtifactKind::Skill && ctx.fs.is_dir(path) {
        collect_skill_files_with(path, 0, ctx)?
    } else {
        Vec::new()
    };

    Ok(ArtifactInfo {
        name: name.to_string(),
        kind,
        scope,
        path: path.to_path_buf(),
        version,
        installed_at,
        source_display,
        source_checksum,
        installed_checksum,
        disk_checksum,
        locally_modified,
        untracked,
        deprecation,
        available_version,
        skill_files,
    })
}

pub(crate) fn collect_skill_files_with(
    dir: &Path,
    indent_level: usize,
    ctx: &AppContext<'_>,
) -> Result<Vec<SkillFileEntry>> {
    let mut entries = ctx.fs.read_dir(dir)?;
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    let mut result = Vec::new();
    for entry in entries {
        let name_str = &entry.file_name;
        if name_str.starts_with('.') {
            continue;
        }

        result.push(SkillFileEntry {
            name: name_str.clone(),
            is_dir: entry.is_dir,
            indent_level,
        });

        if entry.is_dir {
            let sub = collect_skill_files_with(&entry.path, indent_level + 1, ctx)?;
            result.extend(sub);
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Print (no business logic)
// ---------------------------------------------------------------------------

fn print_info(info: &ArtifactInfo) {
    println!("Name:        {}", info.name);
    println!("Type:        {}", info.kind);
    println!("Scope:       {}", info.scope);
    println!("Path:        {}", info.path.display());

    if let Some(v) = &info.version {
        println!("Version:     {v}");
    }
    if let Some(at) = &info.installed_at {
        println!("Installed:   {at}");
    }
    if let Some(src) = &info.source_display {
        println!("Source:      {src}");
    }
    if let Some(cs) = &info.source_checksum {
        println!("Source SHA:  {cs}");
    }
    if let Some(cs) = &info.installed_checksum {
        println!("Install SHA: {cs}");
    }
    if info.locally_modified {
        let disk_cs = info.disk_checksum.as_deref().unwrap_or("unknown");
        println!("Disk SHA:    {disk_cs}  (locally modified)");
    }
    if info.untracked {
        println!("Lock entry:  (none — untracked)");
    }

    if let Some(dep) = &info.deprecation {
        println!("Status:      DEPRECATED");
        if let Some(reason) = &dep.reason {
            println!("  Reason:    {reason}");
        }
        if let Some(repl) = &dep.replacement {
            println!("  Replace:   {repl}");
        }
    }
    if let Some(v) = &info.available_version {
        println!("Available:   v{v} (update available)");
    }

    if !info.skill_files.is_empty() {
        println!();
        println!("Files:");
        for entry in &info.skill_files {
            let indent = "  ".repeat(entry.indent_level + 1);
            if entry.is_dir {
                println!("{indent}{}/", entry.name);
            } else {
                println!("{indent}{}", entry.name);
            }
        }
    }
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
    use crate::test_support::{
        agent_content, install_agent_on_disk, make_ctx, setup_source_with_agent, test_paths,
    };
    use crate::types::{
        ArtifactKind, Deprecation, LockEntry, LockFile, LockSource, SourceEntry, SourceType,
        SourcesFile,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

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

    // --- gather_info_with ---

    #[test]
    fn gather_info_tracked_agent_has_correct_fields() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        // Compute the actual checksum to make it match
        let install_dir = paths.install_dir(ArtifactKind::Agent, false);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Use a checksum that matches the content (we'll rely on the file being there)
        write_lock_entry(&fs, &paths, "my-agent", ArtifactKind::Agent, false, "sha256:somecheck");

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let info = gather_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx).unwrap();

        assert_eq!(info.name, "my-agent");
        assert_eq!(info.kind, ArtifactKind::Agent);
        assert_eq!(info.scope, "global");
        assert_eq!(info.path, path);
        assert_eq!(info.version.as_deref(), Some("1.0.0"));
        assert!(info.installed_at.is_some());
        assert!(info.source_display.is_some());
        assert!(!info.untracked);
    }

    #[test]
    fn gather_info_untracked_agent_sets_untracked_flag() {
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
        let info = gather_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx).unwrap();

        assert!(info.untracked, "expected untracked flag to be set");
        assert!(info.version.is_none());
        assert!(info.installed_at.is_none());
        assert!(info.source_display.is_none());
    }

    #[test]
    fn gather_info_locally_modified_sets_flag_and_disk_checksum() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Install with some content
        install_agent_on_disk(&fs, &paths, "my-agent", "original content", false);

        let install_dir = paths.install_dir(ArtifactKind::Agent, false);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Write a lock entry with a different checksum (simulating modification)
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: Utc::now().to_rfc3339(),
                source: LockSource {
                    repo: "my-source".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:original".to_string(),
                // Installed checksum does NOT match disk content
                installed_checksum: "sha256:different_from_disk".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let info = gather_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx).unwrap();

        assert!(info.locally_modified, "expected locally_modified to be true");
        assert!(info.disk_checksum.is_some(), "expected disk_checksum to be present");
    }

    #[test]
    fn gather_info_deprecation_from_source() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        let install_dir = paths.install_dir(ArtifactKind::Agent, false);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Setup sources with a deprecated agent
        let sources = SourcesFile {
            version: 1,
            sources: {
                let mut m = BTreeMap::new();
                m.insert(
                    "my-source".to_string(),
                    SourceEntry {
                        source_type: SourceType::Local,
                        path: Some(PathBuf::from("/sources/my-source")),
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

        // Deprecated agent in source (uses flat deprecation fields, not YAML block)
        let deprecated_content = "---\nname: my-agent\ndescription: A test agent\ndeprecated: true\ndeprecated_reason: Too old\ndeprecated_replacement: new-agent\n---\n";
        fs.add_file("/sources/my-source/my-agent.md", deprecated_content);

        // Empty lock file
        let lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let info = gather_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx).unwrap();

        assert!(info.deprecation.is_some(), "expected deprecation to be present");
        let dep = info.deprecation.unwrap();
        assert_eq!(dep.reason.as_deref(), Some("Too old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-agent"));
    }

    #[test]
    fn gather_info_available_version_when_source_differs() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        let install_dir = paths.install_dir(ArtifactKind::Agent, false);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Lock entry with version 1.0.0
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: Utc::now().to_rfc3339(),
                source: LockSource {
                    repo: "my-source".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:old".to_string(),
                installed_checksum: "sha256:old".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

        // Source has version 2.0.0
        let sources = SourcesFile {
            version: 1,
            sources: {
                let mut m = BTreeMap::new();
                m.insert(
                    "my-source".to_string(),
                    SourceEntry {
                        source_type: SourceType::Local,
                        path: Some(PathBuf::from("/sources/my-source")),
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
            "/sources/my-source/my-agent.md",
            "---\nname: my-agent\ndescription: A test agent\nversion: 2.0.0\n---\n",
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let info = gather_info_with("my-agent", ArtifactKind::Agent, false, &path, &ctx).unwrap();

        assert_eq!(
            info.available_version.as_deref(),
            Some("2.0.0"),
            "expected available version 2.0.0"
        );
    }

    // --- collect_skill_files_with ---

    #[test]
    fn collect_skill_files_returns_entries_for_nested_structure() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        fs.add_file("/skills/my-skill/lib/helper.sh", "#!/bin/bash");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/my-skill"), 0, &ctx).unwrap();

        assert!(!result.is_empty(), "expected entries for nested skill");
        let names: Vec<&str> = result.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"SKILL.md"), "expected SKILL.md in entries");
        assert!(names.contains(&"lib"), "expected lib/ dir in entries");

        // Verify indent levels
        let skill_md = result.iter().find(|e| e.name == "SKILL.md").unwrap();
        assert_eq!(skill_md.indent_level, 0);
        let helper = result.iter().find(|e| e.name == "helper.sh").unwrap();
        assert_eq!(helper.indent_level, 1);
    }

    #[test]
    fn collect_skill_files_empty_dir_returns_empty_vec() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_dir("/skills/empty-skill");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/empty-skill"), 0, &ctx).unwrap();

        assert!(result.is_empty(), "expected empty vec for empty skill dir");
    }

    #[test]
    fn collect_skill_files_skips_dotfiles() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        fs.add_file("/skills/my-skill/.hidden", "hidden");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/my-skill"), 0, &ctx).unwrap();

        let names: Vec<&str> = result.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&".hidden"), "dotfiles should be skipped");
    }

    #[test]
    fn collect_skill_files_marks_dirs_correctly() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        fs.add_file("/skills/my-skill/lib/tool.py", "code");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/my-skill"), 0, &ctx).unwrap();

        let lib_entry = result.iter().find(|e| e.name == "lib").unwrap();
        assert!(lib_entry.is_dir, "lib/ should be marked as a directory");

        let skill_md = result.iter().find(|e| e.name == "SKILL.md").unwrap();
        assert!(!skill_md.is_dir, "SKILL.md should not be marked as a directory");
    }

    // --- Deprecation struct accessible from tests ---

    #[test]
    fn deprecation_fields_accessible() {
        let dep = Deprecation {
            reason: Some("Old".to_string()),
            replacement: Some("new-agent".to_string()),
        };
        assert_eq!(dep.reason.as_deref(), Some("Old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-agent"));
    }
}
