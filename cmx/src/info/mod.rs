use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::platform::Platform;
use crate::source_iter;
use crate::source_update;
use crate::types::{ArtifactKind, Deprecation, InstallScope};

mod summary;
pub use summary::summarize;

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
    /// The artifact's `description` frontmatter — for a skill this is its
    /// **activation trigger** (the "use this when…" the assistant reads to decide
    /// whether to load it); for an agent, its role description.
    pub activates_when: Option<String>,
    /// An LLM-generated paragraph describing what the artifact does. Populated
    /// only by an `llm`-feature build (see [`summarize`]); `None` otherwise.
    pub summary: Option<String>,
    /// Why a summary is absent *after an attempt was made* — e.g. the content
    /// couldn't be read, or the provider failed. `None` in a lean build (no
    /// attempt) or on success. Lets the display name the real reason instead of
    /// always blaming the provider.
    pub summary_error: Option<String>,
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

pub fn info(name: &str, ctx: &AppContext<'_>) -> Result<ArtifactInfo> {
    // Search both kinds across every platform.
    for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
        if let Some(info) = find_and_gather(name, kind, ctx)? {
            return Ok(info);
        }
    }

    bail!("No installed artifact named '{name}' found.");
}

/// Like [`info`], but scoped to a single kind — backs `cmx skill info` /
/// `cmx agent info`, which know which kind the user meant.
pub fn info_for_kind(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<ArtifactInfo> {
    match find_and_gather(name, kind, ctx)? {
        Some(info) => Ok(info),
        None => bail!("No installed {kind} named '{name}' found."),
    }
}

/// Locate an installed artifact across **every** platform (active platform
/// first, then the rest), returning its gathered info. This mirrors `cmx doctor`,
/// which surveys all platforms — without it, `info` only sees the active
/// `--platform` (Claude by default) and can't describe a skill that lives in,
/// say, another tool's directory (e.g. an `external` skill under `~/.hermes`).
fn find_and_gather(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<Option<ArtifactInfo>> {
    let active = ctx.paths.platform;
    let platforms =
        std::iter::once(active).chain(Platform::ALL.iter().copied().filter(|&p| p != active));

    for platform in platforms {
        let pv = ctx.paths.with_platform(platform);
        if let Some((path, scope)) = config::find_installed_path(name, kind, ctx.fs, &pv) {
            // Gather against the platform the artifact actually lives in, so its
            // (per-platform) lock file is the one consulted.
            let pv_ctx = AppContext {
                fs: ctx.fs,
                git: ctx.git,
                clock: ctx.clock,
                paths: &pv,
                llm: ctx.llm,
            };
            return Ok(Some(gather_info(name, kind, scope, &path, &pv_ctx)?));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Gather (pure logic, no println!)
// ---------------------------------------------------------------------------

pub(crate) fn gather_info(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    path: &Path,
    ctx: &AppContext<'_>,
) -> Result<ArtifactInfo> {
    let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
    let installed = config::installed_single_with_lock_data(name, &lock, kind);
    let lock_entry = installed.as_ref().and_then(|ia| ia.lock_entry);

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
    source_update::ensure_fresh(ctx)?;
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

    let activates_when = read_description(kind, path, ctx);

    Ok(ArtifactInfo {
        name: name.to_string(),
        kind,
        scope: scope.label(),
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
        activates_when,
        summary: None,
        summary_error: None,
    })
}

/// Read the artifact's `description` frontmatter from its content file — the
/// skill's activation trigger or the agent's role description. Returns `None`
/// when the file is unreadable or has no `description`.
fn read_description(kind: ArtifactKind, path: &Path, ctx: &AppContext<'_>) -> Option<String> {
    let content = ctx.fs.read_to_string(&kind.content_path(path)).ok()?;
    let (frontmatter, _) = crate::scan::split_frontmatter_and_body(&content);
    crate::scan::extract_field(&frontmatter?, "description")
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
        // Suppress dotfiles and transient dependency/metadata dirs (node_modules,
        // __pycache__, .venv, …) — the same filter the checksum/copy walkers use,
        // so `info`'s tree shows the skill's real contents, not vendored noise.
        if name_str.starts_with('.') || crate::fs_util::is_transient(name_str) {
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
    use crate::gateway::fakes::FakeFilesystem;
    use crate::lockfile;
    use crate::test_support::{
        TestContext, agent_content, deprecated_agent_content, install_agent_on_disk,
        make_lock_entry_with_checksum, save_lock_with_entry, setup_empty_sources, setup_source,
        setup_source_with_agent, setup_source_with_versioned_agent,
    };
    use crate::types::{ArtifactKind, Deprecation, InstallScope, LockFile};
    use std::collections::BTreeMap;

    fn minimal_info(name: &str, kind: ArtifactKind) -> ArtifactInfo {
        ArtifactInfo {
            name: name.to_string(),
            kind,
            scope: "global",
            path: PathBuf::from(format!("{name}.md")),
            version: None,
            installed_at: None,
            source_display: None,
            source_checksum: None,
            installed_checksum: None,
            disk_checksum: None,
            locally_modified: false,
            untracked: false,
            deprecation: None,
            available_version: None,
            skill_files: vec![],
            activates_when: None,
            summary: None,
            summary_error: None,
        }
    }

    // --- Display for ArtifactInfo ---

    #[test]
    fn artifact_info_display_basic_fields() {
        let info = minimal_info("my-agent", ArtifactKind::Agent);
        let out = info.to_string();
        assert!(out.contains("Name:        my-agent"));
        assert!(out.contains("Type:        agent"));
        assert!(out.contains("Scope:       global"));
    }

    #[test]
    fn artifact_info_display_with_version_and_source() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.version = Some("1.2.3".to_string());
        info.installed_at = Some("2024-01-01T00:00:00Z".to_string());
        info.source_display = Some("guidelines".to_string());
        let out = info.to_string();
        assert!(out.contains("Version:     1.2.3"));
        assert!(out.contains("Installed:   2024-01-01T00:00:00Z"));
        assert!(out.contains("Source:      guidelines"));
    }

    #[test]
    fn artifact_info_display_locally_modified() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.locally_modified = true;
        info.disk_checksum = Some("sha256:abcdef".to_string());
        let out = info.to_string();
        assert!(out.contains("locally modified"));
        assert!(out.contains("sha256:abcdef"));
    }

    #[test]
    fn artifact_info_display_untracked() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.untracked = true;
        let out = info.to_string();
        assert!(out.contains("untracked"));
    }

    #[test]
    fn artifact_info_display_deprecated_with_reason_and_replacement() {
        let mut info = minimal_info("old-agent", ArtifactKind::Agent);
        info.deprecation = Some(Deprecation {
            reason: Some("Too old".to_string()),
            replacement: Some("new-agent".to_string()),
        });
        let out = info.to_string();
        assert!(out.contains("DEPRECATED"));
        assert!(out.contains("Too old"));
        assert!(out.contains("new-agent"));
    }

    #[test]
    fn artifact_info_display_with_available_version() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.available_version = Some("2.0.0".to_string());
        let out = info.to_string();
        assert!(out.contains("v2.0.0"));
        assert!(out.contains("update available"));
    }

    #[test]
    fn artifact_info_display_with_skill_files() {
        let mut info = minimal_info("my-skill", ArtifactKind::Skill);
        info.skill_files = vec![
            SkillFileEntry {
                name: "SKILL.md".to_string(),
                is_dir: false,
                indent_level: 0,
            },
            SkillFileEntry {
                name: "tools".to_string(),
                is_dir: true,
                indent_level: 0,
            },
            SkillFileEntry {
                name: "helper.py".to_string(),
                is_dir: false,
                indent_level: 1,
            },
        ];
        let out = info.to_string();
        assert!(out.contains("Files:"));
        assert!(out.contains("SKILL.md"));
        assert!(out.contains("tools/"));
        assert!(out.contains("helper.py"));
    }

    fn write_lock_entry(
        fs: &FakeFilesystem,
        paths: &crate::paths::ConfigPaths,
        name: &str,
        kind: ArtifactKind,
        scope: InstallScope,
        source_checksum: &str,
    ) {
        let entry = make_lock_entry_with_checksum(
            kind,
            Some("1.0.0"),
            "my-source",
            &format!("{name}.md"),
            source_checksum,
        );
        save_lock_with_entry(fs, paths, name, entry, scope);
    }

    // --- info_with ---

    #[test]
    fn info_finds_global_agent() {
        let t = TestContext::new();

        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &agent_content("my-agent", "test"),
            InstallScope::Global,
        );

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = info("my-agent", &ctx);

        assert!(result.is_ok(), "expected Ok for global agent: {:?}", result.err());
    }

    #[test]
    fn info_finds_local_agent() {
        let t = TestContext::new();

        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &agent_content("my-agent", "test"),
            InstallScope::Local,
        );

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = info("my-agent", &ctx);

        assert!(result.is_ok(), "expected Ok for local agent: {:?}", result.err());
    }

    #[test]
    fn info_errors_when_not_found() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = info("nonexistent-agent", &ctx);

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
        let t = TestContext::new();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        // Compute the actual checksum to make it match
        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Use a checksum that matches the content (we'll rely on the file being there)
        write_lock_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            ArtifactKind::Agent,
            InstallScope::Global,
            "sha256:somecheck",
        );

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let info = gather_info("my-agent", ArtifactKind::Agent, InstallScope::Global, &path, &ctx)
            .unwrap();

        assert_eq!(info.name, "my-agent");
        assert_eq!(info.kind, ArtifactKind::Agent);
        assert_eq!(info.scope, "global");
        assert_eq!(info.path, path);
        assert_eq!(info.version.as_deref(), Some("1.0.0"));
        assert!(info.installed_at.is_some());
        assert!(info.source_display.is_some());
        assert!(!info.untracked);
        // The agent's `description` frontmatter is surfaced as activates_when.
        assert_eq!(info.activates_when.as_deref(), Some("A test agent"));
        assert!(info.summary.is_none(), "summary is only populated by an llm build");
    }

    #[test]
    fn gather_info_untracked_agent_sets_untracked_flag() {
        let t = TestContext::new();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        // No lock entry — untracked
        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);
        let info = gather_info("my-agent", ArtifactKind::Agent, InstallScope::Global, &path, &ctx)
            .unwrap();

        assert!(info.untracked, "expected untracked flag to be set");
        assert!(info.version.is_none());
        assert!(info.installed_at.is_none());
        assert!(info.source_display.is_none());
    }

    #[test]
    fn gather_info_locally_modified_sets_flag_and_disk_checksum() {
        let t = TestContext::new();

        // Install with some content
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            "original content",
            InstallScope::Global,
        );

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global);
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
        save_lock_with_entry(&t.fs, &t.paths, "my-agent", entry, InstallScope::Global);

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let info = gather_info("my-agent", ArtifactKind::Agent, InstallScope::Global, &path, &ctx)
            .unwrap();

        assert!(info.locally_modified, "expected locally_modified to be true");
        assert!(info.disk_checksum.is_some(), "expected disk_checksum to be present");
    }

    #[test]
    fn gather_info_deprecation_from_source() {
        let t = TestContext::new();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Setup sources with a deprecated agent
        setup_source(&t.fs, &t.paths, "my-source", "/sources/my-source");
        // Deprecated agent in source (uses flat deprecation fields, not YAML block)
        t.fs.add_file(
            "/sources/my-source/agents/my-agent.md",
            deprecated_agent_content("my-agent", "A test agent", "Too old", "new-agent"),
        );

        // Empty lock file
        lockfile::save(
            &LockFile {
                version: 1,
                packages: BTreeMap::new(),
            },
            InstallScope::Global,
            &t.fs,
            &t.paths,
        )
        .unwrap();

        let ctx = t.ctx();
        let info = gather_info("my-agent", ArtifactKind::Agent, InstallScope::Global, &path, &ctx)
            .unwrap();

        assert!(info.deprecation.is_some(), "expected deprecation to be present");
        let dep = info.deprecation.unwrap();
        assert_eq!(dep.reason.as_deref(), Some("Too old"));
        assert_eq!(dep.replacement.as_deref(), Some("new-agent"));
    }

    #[test]
    fn gather_info_available_version_when_source_differs() {
        let t = TestContext::new();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global);
        let path = ArtifactKind::Agent.installed_path("my-agent", &install_dir);

        // Lock entry with version 1.0.0
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "my-source",
                "my-agent.md",
                "sha256:old",
            ),
            InstallScope::Global,
        );

        // Source has version 2.0.0
        setup_source_with_versioned_agent(
            &t.fs,
            &t.paths,
            "my-source",
            "/sources/my-source",
            "my-agent",
            "2.0.0",
        );

        let ctx = t.ctx();
        let info = gather_info("my-agent", ArtifactKind::Agent, InstallScope::Global, &path, &ctx)
            .unwrap();

        assert_eq!(
            info.available_version.as_deref(),
            Some("2.0.0"),
            "expected available version 2.0.0"
        );
    }

    // --- collect_skill_files_with ---

    #[test]
    fn collect_skill_files_returns_entries_for_nested_structure() {
        let t = TestContext::new();

        t.fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        t.fs.add_file("/skills/my-skill/lib/helper.sh", "#!/bin/bash");

        let ctx = t.ctx();
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
        let t = TestContext::new();

        t.fs.add_dir("/skills/empty-skill");

        let ctx = t.ctx();
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/empty-skill"), 0, &ctx).unwrap();

        assert!(result.is_empty(), "expected empty vec for empty skill dir");
    }

    #[test]
    fn collect_skill_files_skips_dotfiles() {
        let t = TestContext::new();

        t.fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        t.fs.add_file("/skills/my-skill/.hidden", "hidden");

        let ctx = t.ctx();
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/my-skill"), 0, &ctx).unwrap();

        let names: Vec<&str> = result.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&".hidden"), "dotfiles should be skipped");
    }

    #[test]
    fn collect_skill_files_skips_transient_dependency_dirs() {
        let t = TestContext::new();

        t.fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        t.fs.add_file("/skills/my-skill/scripts/run.py", "code");
        // Vendored/dependency/metadata noise that must not be expanded.
        t.fs.add_file("/skills/my-skill/node_modules/dep/index.js", "x");
        t.fs.add_file("/skills/my-skill/__pycache__/run.cpython-312.pyc", "y");
        t.fs.add_file("/skills/my-skill/.venv/bin/activate", "z");

        let ctx = t.ctx();
        let result =
            collect_skill_files_with(std::path::Path::new("/skills/my-skill"), 0, &ctx).unwrap();
        let names: Vec<&str> = result.iter().map(|e| e.name.as_str()).collect();

        assert!(names.contains(&"SKILL.md"), "real content kept");
        assert!(names.contains(&"scripts"), "real subdir kept");
        assert!(!names.contains(&"node_modules"), "node_modules suppressed");
        assert!(!names.contains(&"__pycache__"), "__pycache__ suppressed");
        assert!(!names.contains(&".venv"), ".venv suppressed");
        // And nothing from inside them leaked via recursion.
        assert!(!names.contains(&"dep"), "node_modules contents not expanded");
        assert!(!names.contains(&"index.js"), "node_modules files not expanded");
    }

    #[test]
    fn collect_skill_files_marks_dirs_correctly() {
        let t = TestContext::new();

        t.fs.add_file("/skills/my-skill/SKILL.md", "---\ndescription: skill\n---\n");
        t.fs.add_file("/skills/my-skill/lib/tool.py", "code");

        let ctx = t.ctx();
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

    // --- activation trigger + kind-scoped lookup ---

    fn install_skill_on_disk(t: &TestContext, name: &str, desc: &str) -> PathBuf {
        let dir = t.paths.install_dir(ArtifactKind::Skill, InstallScope::Global).join(name);
        t.fs.add_file(dir.join("SKILL.md"), crate::test_support::skill_content(desc));
        dir
    }

    #[test]
    fn gather_info_skill_surfaces_activation_trigger() {
        let t = TestContext::new();
        let dir = install_skill_on_disk(&t, "my-skill", "Use this skill when you need X");
        setup_empty_sources(&t.fs, &t.paths);

        let info =
            gather_info("my-skill", ArtifactKind::Skill, InstallScope::Global, &dir, &t.ctx())
                .unwrap();
        assert_eq!(info.activates_when.as_deref(), Some("Use this skill when you need X"));
        assert!(info.summary.is_none());
    }

    #[test]
    fn info_for_kind_finds_skill_and_rejects_wrong_kind() {
        let t = TestContext::new();
        install_skill_on_disk(&t, "my-skill", "desc");
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();

        assert!(info_for_kind("my-skill", ArtifactKind::Skill, &ctx).is_ok());
        let err = info_for_kind("my-skill", ArtifactKind::Agent, &ctx).unwrap_err().to_string();
        assert!(err.contains("agent") && err.contains("my-skill"), "kind-scoped error: {err}");
    }

    #[test]
    fn info_finds_skill_installed_under_another_platform() {
        // The skill lives only under a non-active platform (as an external skill
        // under ~/.hermes does). `info` must survey every platform like `doctor`,
        // not just the active one (Claude in tests) — else it can't describe it.
        let t = TestContext::new();
        let pv = t.paths.with_platform(Platform::Hermes);
        let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).join("productivity");
        t.fs.add_file(dir.join("SKILL.md"), crate::test_support::skill_content("Use when busy"));
        setup_empty_sources(&t.fs, &t.paths);

        let scoped = info_for_kind("productivity", ArtifactKind::Skill, &t.ctx()).unwrap();
        assert_eq!(scoped.name, "productivity");
        assert_eq!(scoped.activates_when.as_deref(), Some("Use when busy"));
        // Found via the both-kinds entry point too.
        assert_eq!(info("productivity", &t.ctx()).unwrap().name, "productivity");
    }

    // --- Display: activation trigger + summary ---

    #[test]
    fn display_skill_shows_activates_when_and_summary() {
        let mut info = minimal_info("my-skill", ArtifactKind::Skill);
        info.activates_when = Some("Use this skill when you need X".to_string());
        info.summary = Some("It does a thing.".to_string());
        let out = info.to_string();
        assert!(out.contains("Activates when:"), "skill activation label: {out}");
        assert!(out.contains("Use this skill when you need X"));
        assert!(out.contains("What it does:"));
        assert!(out.contains("It does a thing."));
    }

    #[test]
    fn display_agent_uses_description_label() {
        let mut info = minimal_info("my-agent", ArtifactKind::Agent);
        info.activates_when = Some("A helpful agent".to_string());
        let out = info.to_string();
        assert!(out.contains("Description:"), "agent uses Description label: {out}");
        assert!(!out.contains("Activates when:"), "agent does not use the skill label");
    }

    #[test]
    fn display_summary_hint_when_no_attempt() {
        // Neither a summary nor an error: no attempt was made (a lean build).
        let info = minimal_info("my-skill", ArtifactKind::Skill);
        let out = info.to_string();
        assert!(out.contains("--features llm"), "lean build hint: {out}");
    }

    #[test]
    fn display_summary_reports_attempt_failure_reason() {
        // A summary was attempted but failed — show the real reason verbatim,
        // not a generic "provider unavailable".
        let mut info = minimal_info("productivity", ArtifactKind::Skill);
        info.summary_error = Some("no readable content to summarize at /x".to_string());
        let out = info.to_string();
        assert!(out.contains("no readable content to summarize"), "names real reason: {out}");
        assert!(!out.contains("--features llm"), "not the lean hint: {out}");
    }
}
