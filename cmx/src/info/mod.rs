//! `cmx info` (artifact detail view).

use crate::error::{CliError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::artifact_status;
use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::platform_iter;
use crate::source_iter;
use crate::types::{ArtifactKind, Deprecation, InstallScope};

mod summary;
pub use summary::summarize;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

fn serialize_path_as_string<S: serde::Serializer>(
    path: &Path,
    s: S,
) -> core::result::Result<S::Ok, S::Error> {
    s.serialize_str(&path.display().to_string())
}

/// All details about a single installed artifact — the canonical shape exposed
/// by `cmx info --json`. Serde attributes encode every field-name decision
/// here so there is exactly one home for the `--json` contract.
#[derive(Debug, Serialize)]
pub struct ArtifactInfo {
    /// The artifact's name.
    pub name: String,
    /// Whether this is an agent or a skill.
    pub kind: ArtifactKind,
    /// The install scope label (`"global"` or `"local"`) this copy was found in.
    pub scope: &'static str,
    /// Serialized via `Path::display()` for lossless UTF-8 representation.
    #[serde(serialize_with = "serialize_path_as_string")]
    pub path: PathBuf,
    /// The version recorded in the lock file, if tracked.
    pub version: Option<String>,
    /// Timestamp the artifact was installed, from the lock entry.
    pub installed_at: Option<String>,
    /// In JSON output this appears as `"source"`.
    #[serde(rename = "source")]
    pub source_display: Option<String>,
    /// The checksum recorded for the source copy at install time.
    pub source_checksum: Option<String>,
    /// The checksum recorded for the installed copy at install time.
    pub installed_checksum: Option<String>,
    /// The artifact's current on-disk checksum, computed when locally modified.
    pub disk_checksum: Option<String>,
    /// `true` when the on-disk copy no longer matches the lock's recorded checksum.
    pub locally_modified: bool,
    /// `true` when no lock entry exists for this artifact.
    pub untracked: bool,
    /// Deprecation notice declared by the source, if any.
    pub deprecation: Option<Deprecation>,
    /// A newer version available from the source, if one exists.
    pub available_version: Option<String>,
    /// In JSON output this appears as `"files"`.
    #[serde(rename = "files")]
    pub skill_files: Vec<SkillFileEntry>,
    /// The artifact's `description` frontmatter — for a skill this is its
    /// **activation trigger** (the "use this when…" the assistant reads to decide
    /// whether to load it); for an agent, its role description.
    /// In JSON output this appears as `"activation_description"`.
    #[serde(rename = "activation_description")]
    pub activates_when: Option<String>,
    /// An LLM-generated paragraph describing what the artifact does. Populated
    /// only by an `llm`-feature build (see [`summarize`]); `None` otherwise.
    pub summary: Option<String>,
    /// Why a summary is absent *after an attempt was made* — e.g. the content
    /// couldn't be read, or the provider failed. `None` in a lean build (no
    /// attempt) or on success. Lets the display name the real reason instead of
    /// always blaming the provider.
    ///
    /// Deliberately absent from `--json` output: it is human-display-only
    /// (the `summary` field is `null` regardless of why it is missing).
    #[serde(skip_serializing)]
    pub summary_error: Option<String>,
}

/// One file or directory within a skill's directory tree, used to render the
/// `--files` listing.
#[derive(Debug, Serialize)]
pub struct SkillFileEntry {
    /// The file or directory's name.
    pub name: String,
    /// `true` when this entry is a directory.
    pub is_dir: bool,
    /// Depth within the skill's directory tree, for indenting the display.
    pub indent_level: usize,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Find an installed artifact by name (searching both agents and skills across
/// every platform) and gather its full detail view for `cmx info`.
pub fn info(name: &str, ctx: &AppContext<'_>) -> Result<ArtifactInfo> {
    // Search both kinds across every platform.
    for kind in [ArtifactKind::Agent, ArtifactKind::Skill] {
        if let Some(info) = find_and_gather(name, kind, ctx)? {
            return Ok(info);
        }
    }

    Err(CliError::ArtifactNotFound {
        name: name.to_string(),
        hint: crate::suggestions::installed_artifact_hint(name, None, ctx),
    })
}

/// Like [`info`], but scoped to a single kind — backs `cmx skill info` /
/// `cmx agent info`, which know which kind the user meant.
pub fn info_for_kind(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<ArtifactInfo> {
    match find_and_gather(name, kind, ctx)? {
        Some(info) => Ok(info),
        None => Err(CliError::ArtifactNotInstalled {
            kind,
            name: name.to_string(),
            hint: crate::suggestions::installed_artifact_hint(name, Some(kind), ctx),
        }),
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
    for view in platform_iter::views_for(ctx.paths, platform_iter::active_first(active), kind) {
        if let Some((path, scope)) = config::find_installed_path(name, kind, ctx.fs, &view.paths) {
            // Gather against the platform the artifact actually lives in, so its
            // (per-platform) lock file is the one consulted.
            let pv_ctx = ctx.with_paths(&view.paths);
            return Ok(Some(gather_info(name, kind, scope, &path, &pv_ctx)?));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Gather (pure logic, no println!)
// ---------------------------------------------------------------------------

struct LockDerived {
    version: Option<String>,
    installed_at: Option<String>,
    source_display: Option<String>,
    source_checksum: Option<String>,
    installed_checksum: Option<String>,
    disk_checksum: Option<String>,
    locally_modified: bool,
    untracked: bool,
}

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

    let ld = if let Some(entry) = lock_entry {
        let (locally_modified, disk_checksum) =
            checksum::current_checksum_if_modified(path, kind, entry, ctx.fs)?;
        LockDerived {
            version: entry.version.clone(),
            installed_at: Some(entry.installed_at.clone()),
            source_display: Some(format!("{} ({})", entry.source.repo, entry.source.path)),
            source_checksum: Some(entry.source_checksum.clone()),
            installed_checksum: Some(entry.installed_checksum.clone()),
            disk_checksum,
            locally_modified,
            untracked: false,
        }
    } else {
        LockDerived {
            version: None,
            installed_at: None,
            source_display: None,
            source_checksum: None,
            installed_checksum: None,
            disk_checksum: None,
            locally_modified: false,
            untracked: true,
        }
    };

    // Check source for deprecation and available version
    let mut deprecation: Option<Deprecation> = None;
    let mut available_version: Option<String> = None;

    for sa in source_iter::find_by_name_and_kind(name, kind, ctx)? {
        if sa.artifact.deprecation.is_some() {
            deprecation = sa.artifact.deprecation;
        }
        let source_checksum = checksum::checksum_artifact(&sa.artifact.path, kind, ctx.fs)?;
        if artifact_status::source_outdated(
            lock_entry,
            &source_checksum,
            sa.artifact.version.as_deref(),
        ) {
            if let Some(v) = sa.artifact.version.as_deref() {
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
        version: ld.version,
        installed_at: ld.installed_at,
        source_display: ld.source_display,
        source_checksum: ld.source_checksum,
        installed_checksum: ld.installed_checksum,
        disk_checksum: ld.disk_checksum,
        locally_modified: ld.locally_modified,
        untracked: ld.untracked,
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
    use crate::platform::Platform;
    use crate::test_support::{
        TestContext, agent_content, deprecated_agent_content, install_agent_on_disk,
        make_lock_entry_with_checksum, save_lock_with_entry, setup_empty_sources, setup_source,
        setup_source_with_agent, setup_source_with_versioned_agent,
    };
    use crate::types::{ArtifactKind, Deprecation, InstallScope, LockFile};
    use std::collections::BTreeMap;

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
        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        let path = ArtifactKind::Agent.installed_path(
            "my-agent",
            &install_dir,
            ArtifactKind::HOME_AGENT_EXT,
        );

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
        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        let path = ArtifactKind::Agent.installed_path(
            "my-agent",
            &install_dir,
            ArtifactKind::HOME_AGENT_EXT,
        );
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

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        let path = ArtifactKind::Agent.installed_path(
            "my-agent",
            &install_dir,
            ArtifactKind::HOME_AGENT_EXT,
        );

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

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        let path = ArtifactKind::Agent.installed_path(
            "my-agent",
            &install_dir,
            ArtifactKind::HOME_AGENT_EXT,
        );

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
    fn gather_info_no_available_version_when_source_checksum_and_version_match_lock() {
        // When the lock source_checksum and version match the source, source_outdated
        // returns false and available_version should remain None.
        let t = TestContext::new();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        let path = ArtifactKind::Agent.installed_path(
            "my-agent",
            &install_dir,
            ArtifactKind::HOME_AGENT_EXT,
        );

        // Set up the source with a known version so we can record the exact checksum
        setup_source_with_versioned_agent(
            &t.fs,
            &t.paths,
            "my-source",
            "/sources/my-source",
            "my-agent",
            "1.0.0",
        );

        // Compute the source checksum so the lock entry matches exactly
        let source_path = std::path::PathBuf::from("/sources/my-source/agents/my-agent.md");
        let source_cs =
            crate::checksum::checksum_artifact(&source_path, ArtifactKind::Agent, &t.fs).unwrap();

        // Lock entry with version 1.0.0 and the matching source checksum
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_with_checksum(
                ArtifactKind::Agent,
                Some("1.0.0"),
                "my-source",
                "agents/my-agent.md",
                &source_cs,
            ),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let info = gather_info("my-agent", ArtifactKind::Agent, InstallScope::Global, &path, &ctx)
            .unwrap();

        assert!(
            info.available_version.is_none(),
            "available_version should be None when source checksum+version match lock entry, \
             got: {:?}",
            info.available_version
        );
    }

    #[test]
    fn gather_info_available_version_when_source_differs() {
        let t = TestContext::new();

        let content = agent_content("my-agent", "A test agent");
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);

        let install_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
        let path = ArtifactKind::Agent.installed_path(
            "my-agent",
            &install_dir,
            ArtifactKind::HOME_AGENT_EXT,
        );

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
        let dir = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join(name);
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
        let dir = pv
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("productivity");
        t.fs.add_file(dir.join("SKILL.md"), crate::test_support::skill_content("Use when busy"));
        setup_empty_sources(&t.fs, &t.paths);

        let scoped = info_for_kind("productivity", ArtifactKind::Skill, &t.ctx()).unwrap();
        assert_eq!(scoped.name, "productivity");
        assert_eq!(scoped.activates_when.as_deref(), Some("Use when busy"));
        // Found via the both-kinds entry point too.
        assert_eq!(info("productivity", &t.ctx()).unwrap().name, "productivity");
    }
}
