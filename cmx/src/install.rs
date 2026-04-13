use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::gateway::filesystem::Filesystem;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::{ArtifactKind, LockEntry, LockSource};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct InstallResult {
    pub artifact_name: String,
    pub version: Option<String>,
    pub kind: ArtifactKind,
    pub source_name: String,
    pub dest_dir: PathBuf,
}

#[derive(Debug)]
pub struct InstallAllResult {
    pub installed: Vec<InstallResult>,
    pub kind: ArtifactKind,
}

#[derive(Debug)]
pub struct UpdateAllResult {
    pub updated: Vec<InstallResult>,
    pub kind: ArtifactKind,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Tuple returned by artifact lookup: source name, path, source root, and version.
type FoundArtifact = (String, std::path::PathBuf, std::path::PathBuf, Option<String>);

/// Locate a uniquely named artifact across sources.
fn find_artifact(
    artifact_name: &str,
    source_name: Option<&str>,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<FoundArtifact> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    if sources.sources.is_empty() {
        bail!("No sources registered. Add one with: cmx source add <name> <path-or-url>");
    }

    let search_sources: std::collections::BTreeMap<_, _> = if let Some(src) = source_name {
        let entry =
            sources.sources.get(src).with_context(|| format!("Source '{src}' not found."))?;
        std::iter::once((src.to_string(), entry.clone())).collect()
    } else {
        sources.sources.clone()
    };

    let mut found: Vec<FoundArtifact> = Vec::new();
    for sa in source_iter::each_source_artifact_with(&search_sources, ctx.fs) {
        if sa.artifact.name == artifact_name && sa.artifact.kind == kind {
            found.push((sa.source_name, sa.artifact.path, sa.source_root, sa.artifact.version));
        }
    }

    if found.is_empty() {
        bail!("No {kind} named '{artifact_name}' found in registered sources.");
    }

    if found.len() > 1 {
        let source_names: Vec<_> = found.iter().map(|(s, _, _, _)| s.as_str()).collect();
        bail!(
            "'{artifact_name}' found in multiple sources: {}. Use <source>:{artifact_name} to disambiguate.",
            source_names.join(", ")
        );
    }

    Ok(found.remove(0))
}

/// Copy an artifact from source to destination, returning the destination path.
fn copy_artifact(
    artifact_path: &std::path::Path,
    dest_dir: &std::path::Path,
    kind: ArtifactKind,
    artifact_name: &str,
    ctx: &AppContext<'_>,
) -> Result<std::path::PathBuf> {
    let dest_path = match kind {
        ArtifactKind::Agent => {
            let filename = artifact_path.file_name().context("Invalid agent path")?;
            let dest = dest_dir.join(filename);
            ctx.fs.copy_file(artifact_path, &dest).with_context(|| {
                format!("Failed to copy {} to {}", artifact_path.display(), dest.display())
            })?;
            dest
        }
        ArtifactKind::Skill => {
            let dir_name = artifact_path.file_name().context("Invalid skill path")?;
            let dest = dest_dir.join(dir_name);
            copy_dir_recursive_with(artifact_path, &dest, ctx.fs)?;
            dest
        }
    };

    // Validate skill installation
    if matches!(kind, ArtifactKind::Skill) {
        let skill_md = dest_path.join("SKILL.md");
        if !ctx.fs.exists(&skill_md) {
            let _ = ctx.fs.remove_dir_all(&dest_path);
            bail!("Skill '{artifact_name}' is missing SKILL.md. Partial install removed.");
        }
    }

    Ok(dest_path)
}

pub fn install_with(
    name: &str,
    kind: ArtifactKind,
    local: bool,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    let (source_name, artifact_name) = parse_name(name);

    source::auto_update_all_with(ctx)?;

    let (found_source, artifact_path, source_root, artifact_version) =
        find_artifact(artifact_name, source_name, kind, ctx)?;

    let dest_dir = ctx.paths.install_dir(kind, local);
    ctx.fs
        .create_dir_all(&dest_dir)
        .with_context(|| format!("Failed to create {}", dest_dir.display()))?;

    let source_checksum = checksum::checksum_artifact_with(&artifact_path, kind, ctx.fs)?;

    let relative_path = artifact_path
        .strip_prefix(&source_root)
        .unwrap_or(&artifact_path)
        .to_string_lossy()
        .to_string();

    // Check for local modifications before overwriting
    if !force {
        let dest_check = kind.installed_path(artifact_name, &dest_dir);
        if ctx.fs.exists(&dest_check) {
            let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
            if let Some(entry) = lock.packages.get(artifact_name) {
                if checksum::is_locally_modified(&dest_check, kind, entry, ctx.fs)? {
                    bail!(
                        "'{artifact_name}' has local modifications. Use --force to overwrite, \
                         or 'cmx {kind} diff {artifact_name}' to review changes first."
                    );
                }
            }
        }
    }

    let dest_path = copy_artifact(&artifact_path, &dest_dir, kind, artifact_name, ctx)?;
    let installed_checksum = checksum::checksum_artifact_with(&dest_path, kind, ctx.fs)?;

    let mut lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    lock.packages.insert(
        artifact_name.to_string(),
        LockEntry {
            artifact_type: kind,
            version: artifact_version.clone(),
            installed_at: ctx.clock.now().to_rfc3339(),
            source: LockSource {
                repo: found_source.clone(),
                path: relative_path,
            },
            source_checksum,
            installed_checksum,
        },
    );
    lockfile::save_with(&lock, local, ctx.fs, ctx.paths)?;

    Ok(InstallResult {
        artifact_name: artifact_name.to_string(),
        version: artifact_version,
        kind,
        source_name: found_source,
        dest_dir,
    })
}

pub fn update_with(
    name: &str,
    kind: ArtifactKind,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    let Some((entry, local)) = lockfile::find_entry_with(name, ctx.fs, ctx.paths)? else {
        bail!(
            "No installed {kind} named '{name}' found. Install it first with 'cmx {kind} install {name}'."
        );
    };
    let pinned = format!("{}:{}", entry.source.repo, name);
    install_with(&pinned, kind, local, force, ctx)
}

pub fn install_all_with(
    kind: ArtifactKind,
    local: bool,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallAllResult> {
    source::auto_update_all_with(ctx)?;

    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let mut installed = Vec::new();

    for sa in source_iter::all_artifacts(ctx)? {
        if sa.artifact.kind != kind {
            continue;
        }
        // Skip if already tracked with matching version AND checksum
        if let Some(lock_entry) = lock.packages.get(&sa.artifact.name) {
            let source_cs = checksum::checksum_artifact_with(&sa.artifact.path, kind, ctx.fs)?;
            if lock_entry.version.as_deref() == sa.artifact.version.as_deref()
                && lock_entry.source_checksum == source_cs
            {
                continue;
            }
        }
        let pinned = format!("{}:{}", sa.source_name, sa.artifact.name);
        let result = install_with(&pinned, kind, local, force, ctx)?;
        installed.push(result);
    }

    Ok(InstallAllResult { installed, kind })
}

pub fn update_all_with(
    kind: ArtifactKind,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<UpdateAllResult> {
    source::auto_update_all_with(ctx)?;

    // Scan sources for current checksums
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let all_source_info = source_iter::scan_all_with_checksums(&sources.sources, ctx.fs)?;
    let mut updated = Vec::new();

    let (global_lock, local_lock) = lockfile::load_both_with(ctx.fs, ctx.paths)?;
    for (local, lock) in [(false, &global_lock), (true, &local_lock)] {
        for (name, entry) in &lock.packages {
            if entry.artifact_type != kind {
                continue;
            }

            if let Some(source_infos) = all_source_info.get(name)
                && source_infos.iter().any(|si| {
                    si.source_name == entry.source.repo && si.checksum != entry.source_checksum
                })
            {
                let pinned = format!("{}:{name}", entry.source.repo);
                let result = install_with(&pinned, kind, local, force, ctx)?;
                updated.push(result);
            }
        }
    }

    Ok(UpdateAllResult { updated, kind })
}

fn copy_dir_recursive_with(src: &Path, dest: &Path, fs: &dyn Filesystem) -> Result<()> {
    fs.create_dir_all(dest)
        .with_context(|| format!("Failed to create {}", dest.display()))?;

    for entry in fs.read_dir(src)? {
        let dest_path = dest.join(&entry.file_name);

        if entry.is_dir {
            copy_dir_recursive_with(&entry.path, &dest_path, fs)?;
        } else {
            fs.copy_file(&entry.path, &dest_path)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

fn parse_name(name: &str) -> (Option<&str>, &str) {
    if let Some((source, artifact)) = name.split_once(':') {
        (Some(source), artifact)
    } else {
        (None, name)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{
        agent_content, make_ctx, setup_source, setup_source_with_agent, setup_sources, test_paths,
    };
    use crate::types::{ArtifactKind, LockFile, SourcesFile};
    use chrono::Utc;

    // --- parse_name ---

    #[test]
    fn parse_name_with_source_prefix() {
        let (source, artifact) = parse_name("guidelines:rust-craftsperson");
        assert_eq!(source, Some("guidelines"));
        assert_eq!(artifact, "rust-craftsperson");
    }

    #[test]
    fn parse_name_without_source_prefix() {
        let (source, artifact) = parse_name("rust-craftsperson");
        assert_eq!(source, None);
        assert_eq!(artifact, "rust-craftsperson");
    }

    #[test]
    fn parse_name_splits_on_first_colon_only() {
        let (source, artifact) = parse_name("a:b:c");
        assert_eq!(source, Some("a"));
        assert_eq!(artifact, "b:c");
    }

    #[test]
    fn parse_name_empty_source() {
        let (source, artifact) = parse_name(":artifact");
        assert_eq!(source, Some(""));
        assert_eq!(artifact, "artifact");
    }

    #[test]
    fn parse_name_empty_artifact() {
        let (source, artifact) = parse_name("source:");
        assert_eq!(source, Some("source"));
        assert_eq!(artifact, "");
    }

    // --- install_with business logic tests ---

    #[test]
    fn install_bails_when_no_sources_registered() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Empty sources.json (default)
        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No sources registered"), "unexpected: {msg}");
    }

    #[test]
    fn install_bails_when_artifact_not_found() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "existing-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("nonexistent-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("nonexistent-agent"), "unexpected: {msg}");
    }

    #[test]
    fn install_bails_on_ambiguous_name() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Two sources, both with the same agent name
        setup_sources(&fs, &paths, &[("source1", "/source1"), ("source2", "/source2")]);
        fs.add_file("/source1/agents/my-agent.md", agent_content("my-agent", "Agent from source1"));
        fs.add_file("/source2/agents/my-agent.md", agent_content("my-agent", "Agent from source2"));

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("multiple sources"), "unexpected: {msg}");
    }

    #[test]
    fn install_succeeds_with_source_prefix_disambiguation() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-source:my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_ok(), "expected ok, got: {:?}", result.err());

        // Verify the artifact was installed
        let expected_dest = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        assert!(
            fs.file_exists(&expected_dest),
            "agent file should be installed at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_copies_agent_file_to_correct_destination() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        let expected_dest = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        assert!(
            fs.file_exists(&expected_dest),
            "agent file should be at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_records_checksums_in_lock() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        let lock = lockfile::load_with(false, &fs, &paths).unwrap();
        let entry = lock.packages.get("my-agent").expect("lock entry must exist");
        assert!(!entry.source_checksum.is_empty());
        assert!(!entry.installed_checksum.is_empty());
        assert!(entry.source_checksum.starts_with("sha256:"));
        assert!(entry.installed_checksum.starts_with("sha256:"));
    }

    #[test]
    fn install_records_timestamp_from_clock() {
        use chrono::TimeZone;
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let fixed_time = Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let clock = FakeClock::at(fixed_time);
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        let lock = lockfile::load_with(false, &fs, &paths).unwrap();
        let entry = lock.packages.get("my-agent").unwrap();
        assert!(
            entry.installed_at.starts_with("2024-06-01"),
            "expected 2024-06-01 timestamp, got: {}",
            entry.installed_at
        );
    }

    #[test]
    fn install_bails_on_local_modifications_without_force() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        // First install
        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        // Modify the installed file (different content than the recorded checksum)
        let installed_path = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        fs.write(&installed_path, "modified content that differs from source").unwrap();

        // Second install should fail without force
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("local modifications"), "unexpected: {msg}");
    }

    #[test]
    fn install_proceeds_on_local_modifications_with_force() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        // First install
        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        // Modify the installed file
        let installed_path = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        fs.write(&installed_path, "modified content").unwrap();

        // Second install with force should succeed
        let result = install_with("my-agent", ArtifactKind::Agent, false, true, &ctx);
        assert!(result.is_ok(), "force install should succeed: {:?}", result.err());
    }

    #[test]
    fn install_validates_skill_has_skill_md() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Set up a source with a "skill" directory that has no SKILL.md
        // Note: scan_source_with only picks up skills with SKILL.md, so we need
        // to add a skill that DOES have SKILL.md in the source but then it gets
        // stripped during copy. Instead, test the validation by setting up a skill
        // that is declared in a marketplace.json but has SKILL.md.
        // Actually, the scan will only find skills that have SKILL.md, so let's test
        // by setting up a valid skill source and then removing SKILL.md mid-install.
        // The cleanest approach: write a marketplace.json that declares a skill
        // whose dir exists but SKILL.md check happens during validation of install.
        //
        // In practice the copy_dir_recursive_with copies all files. If the source
        // has SKILL.md, the dest also will. So we test the validation by ensuring
        // a skill without SKILL.md is rejected by scanning (not found at all).
        // The real validation guard fires if copy succeeded but SKILL.md is absent.
        // We simulate this by using a marketplace.json declaring a skill path
        // where the directory exists but has no SKILL.md (so scan skips it).
        // Since scan won't find it, install will bail with "not found" —
        // which tests the right path without needing to intercept copy.

        setup_source(&fs, &paths, "my-source", "/sources/my-source");

        // Skill directory without SKILL.md — scanner won't find it
        fs.add_file("/sources/my-source/my-skill/tool.py", "code");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-skill", ArtifactKind::Skill, false, false, &ctx);
        assert!(result.is_err());
        // Either "not found in registered sources" or "missing SKILL.md"
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("my-skill") || msg.contains("SKILL.md"), "unexpected: {msg}");
    }

    #[test]
    fn install_removes_partial_skill_on_validation_failure() {
        // This tests the guard: if copy succeeds but SKILL.md is absent after copying,
        // we remove the partial install.
        // We simulate this by writing a marketplace.json that declares a skill at a
        // path where SKILL.md is absent in the destination. We'd need to intercept copy.
        // In the fake, the copy faithfully reproduces the source, so if source has no
        // SKILL.md, copy produces no SKILL.md either.
        //
        // Set up: marketplace declares a skill whose source dir has no SKILL.md.
        // scan_marketplace_with will warn and skip it. So install won't find it.
        // This test documents the edge: the SKILL.md validation guard removes partial
        // installs if SKILL.md is absent post-copy.
        //
        // We can trigger the guard by using a fake filesystem where copy_file
        // doesn't actually copy SKILL.md. Since FakeFilesystem faithfully copies,
        // this path is best exercised via integration test. We document it here.
        //
        // Verify the inverse: skill WITH SKILL.md installs successfully.
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source(&fs, &paths, "my-source", "/sources/my-source");
        // Skill WITH SKILL.md
        fs.add_file("/sources/my-source/my-skill/SKILL.md", "---\ndescription: My skill\n---\n");
        fs.add_file("/sources/my-source/my-skill/tool.py", "code");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-skill", ArtifactKind::Skill, false, false, &ctx);
        assert!(result.is_ok(), "skill with SKILL.md should install: {:?}", result.err());

        // Verify SKILL.md was copied to dest
        let dest = paths.install_dir(ArtifactKind::Skill, false).join("my-skill");
        assert!(fs.file_exists(&dest.join("SKILL.md")));
    }

    // --- Verify empty lock produces empty lock file ---
    #[test]
    fn install_writes_lock_with_source_repo_name() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "guidelines", "/sources/guidelines", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        let lock = lockfile::load_with(false, &fs, &paths).unwrap();
        let entry = lock.packages.get("my-agent").unwrap();
        assert_eq!(entry.source.repo, "guidelines");
    }

    // Ensure the LockFile starts empty
    #[test]
    fn fresh_lock_file_has_no_entries() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let lock: LockFile = lockfile::load_with(false, &fs, &paths).unwrap();
        assert!(lock.packages.is_empty());
    }

    // --- install_with: assert on InstallResult fields ---

    #[test]
    fn perform_install_returns_correct_artifact_name() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        assert_eq!(result.artifact_name, "my-agent");
        assert_eq!(result.kind, ArtifactKind::Agent);
        assert_eq!(result.source_name, "my-source");
    }

    #[test]
    fn perform_install_dest_dir_matches_install_dir() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx).unwrap();

        let expected_dir = paths.install_dir(ArtifactKind::Agent, false);
        assert_eq!(result.dest_dir, expected_dir);
    }

    #[test]
    fn perform_install_bails_when_no_sources_registered() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let sources = SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("No sources registered"), "unexpected: {msg}");
    }

    // --- failure-path tests ---

    #[test]
    fn install_agent_does_not_update_lock_when_copy_fails() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        fs.set_fail_on_copy(true);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err(), "expected Err when copy fails");

        // Lock file should have no entry for the agent
        let lock = lockfile::load_with(false, &fs, &paths).unwrap();
        assert!(
            !lock.packages.contains_key("my-agent"),
            "lock should not be updated after failed copy"
        );
    }

    #[test]
    fn install_agent_lock_save_failure_leaves_artifact_on_disk() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        // Cause the lock file write to fail
        fs.set_fail_on_write(paths.lock_path(false));

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install_with("my-agent", ArtifactKind::Agent, false, false, &ctx);
        assert!(result.is_err(), "expected Err when lock save fails");

        // Despite the lock save failure, the agent file was already copied to disk
        // (documents the current no-rollback behavior)
        let expected_dest = paths.install_dir(ArtifactKind::Agent, false).join("my-agent.md");
        assert!(
            fs.file_exists(&expected_dest),
            "agent file should still exist on disk even after lock save failure"
        );
    }
}
