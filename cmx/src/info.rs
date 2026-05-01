use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source_iter;
use crate::source_update;
use crate::types::{ArtifactKind, Deprecation};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ArtifactInfo {
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

#[derive(Debug)]
pub struct SkillFileEntry {
    pub name: String,
    pub is_dir: bool,
    pub indent_level: usize,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn info_with(name: &str, ctx: &AppContext<'_>) -> Result<ArtifactInfo> {
    // Search both kinds, global then local for each
    for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
        if let Some((path, local)) = config::find_installed_path(name, kind, ctx.fs, ctx.paths) {
            return gather_info_with(name, kind, local, &path, ctx);
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
        let (locally_modified, disk_checksum) =
            checksum::current_checksum_if_modified(path, kind, entry, ctx.fs)?;
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
    source_update::auto_update_all_with(ctx).ok();
    let mut deprecation: Option<Deprecation> = None;
    let mut available_version: Option<String> = None;

    for sa in source_iter::find_by_name_and_kind(name, kind, ctx)? {
        if sa.artifact.deprecation.is_some() {
            deprecation = sa.artifact.deprecation;
        }
        if let Some(v) = sa.artifact.version.as_deref() {
            let installed_v = lock_entry.and_then(|e| e.version.as_deref());
            if installed_v != Some(v) {
                available_version = Some(v.to_string());
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
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::lockfile;
    use crate::test_support::{
        agent_content, deprecated_agent_content, install_agent_on_disk, make_ctx,
        make_lock_entry_with_checksum, save_lock_with_entry, setup_empty_sources, setup_source,
        setup_source_with_agent, setup_source_with_versioned_agent, test_paths,
    };
    use crate::types::{ArtifactKind, Deprecation, LockFile};
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn write_lock_entry(
        fs: &FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        name: &str,
        kind: ArtifactKind,
        local: bool,
        source_checksum: &str,
    ) {
        let entry = make_lock_entry_with_checksum(
            kind,
            Some("1.0.0"),
            "my-source",
            &format!("{name}.md"),
            source_checksum,
        );
        save_lock_with_entry(fs, paths, name, entry, local);
    }

    // --- info_with ---

    #[test]
    fn info_finds_global_agent() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_agent_on_disk(&fs, &paths, "my-agent", &agent_content("my-agent", "test"), false);

        setup_empty_sources(&fs, &paths);

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

        setup_empty_sources(&fs, &paths);

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

        setup_empty_sources(&fs, &paths);

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
        setup_empty_sources(&fs, &paths);

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
        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "my-source",
            "my-agent.md",
            "sha256:original",
        );
        // Installed checksum does NOT match disk content
        entry.installed_checksum = "sha256:different_from_disk".to_string();
        save_lock_with_entry(&fs, &paths, "my-agent", entry, false);

        setup_empty_sources(&fs, &paths);

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
        setup_source(&fs, &paths, "my-source", "/sources/my-source");
        // Deprecated agent in source (uses flat deprecation fields, not YAML block)
        fs.add_file(
            "/sources/my-source/agents/my-agent.md",
            deprecated_agent_content("my-agent", "A test agent", "Too old", "new-agent"),
        );

        // Empty lock file
        lockfile::save_with(
            &LockFile {
                version: 1,
                packages: BTreeMap::new(),
            },
            false,
            &fs,
            &paths,
        )
        .unwrap();

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
        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "my-source",
                "my-agent.md",
                "sha256:old",
            ),
            false,
        );

        // Source has version 2.0.0
        setup_source_with_versioned_agent(
            &fs,
            &paths,
            "my-source",
            "/sources/my-source",
            "my-agent",
            "2.0.0",
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
