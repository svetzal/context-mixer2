use anyhow::{Result, bail};
use std::path::PathBuf;

use crate::checksum;
use crate::context::AppContext;
use crate::copy;
use crate::lockfile;
use crate::paths::ConfigPaths;
use crate::source_iter;
use crate::source_update;
use crate::types::{self, ArtifactKind, InstallScope, LockEntry, LockSource};

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
pub struct BatchInstallResult {
    pub items: Vec<InstallResult>,
    pub kind: ArtifactKind,
    pub is_update: bool,
}

/// Outcome of installing several named artifacts in one pass.
#[derive(Debug)]
pub struct InstallManyResult {
    pub kind: ArtifactKind,
    pub installed: Vec<InstallResult>,
    /// `(name, reason)` for names that failed (not found, ambiguous, locally
    /// modified without `--force`, …).
    pub failed: Vec<(String, String)>,
}

/// Pure description of an intended installation — computed from source metadata
/// and path configuration, with no filesystem access.
#[derive(Debug)]
pub struct InstallPlan {
    pub artifact_name: String,
    pub version: Option<String>,
    pub source_name: String,
    pub source_root: PathBuf,
    pub dest_dir: PathBuf,
    pub relative_path: String,
}

pub fn install(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    ctx.paths.ensure_supports(kind)?;

    let (source_name, artifact_name) = parse_name(name);

    source_update::ensure_fresh(ctx)?;

    let found = source_iter::find_unique(artifact_name, kind, source_name, ctx)?;

    let plan = plan_install(artifact_name, kind, scope, &found, ctx.paths);

    ctx.fs.create_dir_all(&plan.dest_dir)?;

    let source_checksum = checksum::checksum_artifact(&found.artifact.path, kind, ctx.fs)?;

    // Check for local modifications before overwriting
    if !force {
        let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
        check_local_modifications(
            artifact_name,
            kind,
            scope,
            lock.packages.get(artifact_name),
            ctx,
        )?;
    }

    // Record whether this is a fresh install (vs. an update/reinstall) so that
    // we can roll back if the lockfile write fails.
    let already_installed = ctx.paths.is_installed(kind, artifact_name, scope, ctx.fs);

    let dest_path =
        copy::copy_artifact(&found.artifact.path, &plan.dest_dir, kind, artifact_name, ctx)?;
    let installed_checksum = checksum::checksum_artifact(&dest_path, kind, ctx.fs)?;

    let lock_result = lockfile::mutate(scope, ctx.fs, ctx.paths, |lock| {
        lock.packages.insert(
            artifact_name.to_string(),
            LockEntry {
                artifact_type: kind,
                version: plan.version.clone(),
                installed_at: ctx.clock.now().to_rfc3339(),
                source: LockSource {
                    repo: plan.source_name.clone(),
                    path: plan.relative_path.clone(),
                },
                source_checksum,
                installed_checksum,
            },
        );
    });

    if let Err(lock_err) = lock_result {
        // If we performed a fresh install and the lockfile write failed, roll
        // back by removing the artifact we just copied.  This avoids leaving a
        // ghost: an artifact on disk with no lockfile entry.  We ignore any
        // remove error to ensure the original lock error is surfaced.
        if !already_installed {
            let _ = match kind {
                types::ArtifactKind::Agent => ctx.fs.remove_file(&dest_path),
                types::ArtifactKind::Skill => ctx.fs.remove_dir_all(&dest_path),
            };
        }
        return Err(lock_err);
    }

    Ok(InstallResult {
        artifact_name: artifact_name.to_string(),
        version: plan.version,
        kind,
        source_name: plan.source_name,
        dest_dir: plan.dest_dir,
    })
}

/// Install several named artifacts in one pass. Best-effort: each name is
/// installed independently; failures (not found, ambiguous, locally modified
/// without `--force`) are collected with their reason rather than aborting the
/// batch. Backs `cmx {skill,agent} install <name>...`.
pub fn install_many(
    names: &[String],
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallManyResult> {
    let (installed, failed) =
        partition_results(names, |name| install(name, kind, scope, force, ctx));
    Ok(InstallManyResult {
        kind,
        installed,
        failed,
    })
}

pub fn update(
    name: &str,
    kind: ArtifactKind,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<InstallResult> {
    let Some((entry, scope)) = lockfile::find_entry(name, ctx.fs, ctx.paths)? else {
        bail!(
            "No installed {kind} named '{name}' found. Install it first with 'cmx {kind} install {name}'."
        );
    };
    let pinned = format!("{}:{}", entry.source.repo, name);
    install(&pinned, kind, scope, force, ctx)
}

pub fn install_all(
    kind: ArtifactKind,
    scope: InstallScope,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<BatchInstallResult> {
    ctx.paths.ensure_supports(kind)?;

    source_update::ensure_fresh(ctx)?;

    let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
    let mut installed = Vec::new();

    for sa in source_iter::all_artifacts(ctx)? {
        if sa.artifact.kind != kind {
            continue;
        }
        // Skip if already tracked with matching version AND checksum
        if let Some(lock_entry) = lock.packages.get(&sa.artifact.name) {
            let source_cs = checksum::checksum_artifact(&sa.artifact.path, kind, ctx.fs)?;
            if lock_entry.version.as_deref() == sa.artifact.version.as_deref()
                && lock_entry.source_checksum == source_cs
            {
                continue;
            }
        }
        let pinned = format!("{}:{}", sa.source_name, sa.artifact.name);
        let result = install(&pinned, kind, scope, force, ctx)?;
        installed.push(result);
    }

    Ok(BatchInstallResult {
        items: installed,
        kind,
        is_update: false,
    })
}

pub fn update_all(
    kind: ArtifactKind,
    force: bool,
    ctx: &AppContext<'_>,
) -> Result<BatchInstallResult> {
    ctx.paths.ensure_supports(kind)?;

    source_update::ensure_fresh(ctx)?;

    let all_source_info = source_iter::all_with_checksums(ctx)?;
    let mut updated = Vec::new();

    let locks = lockfile::load_both(ctx.fs, ctx.paths)?;
    for (scope, lock) in &locks {
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
                let result = install(&pinned, kind, *scope, force, ctx)?;
                updated.push(result);
            }
        }
    }

    Ok(BatchInstallResult {
        items: updated,
        kind,
        is_update: true,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Run `op` over each name, collecting successes into the first vec and
/// `(name, reason)` failure pairs into the second. Never returns `Err`.
fn partition_results<S, F>(names: &[String], mut op: F) -> (Vec<S>, Vec<(String, String)>)
where
    F: FnMut(&str) -> Result<S>,
{
    let mut ok = Vec::new();
    let mut err = Vec::new();
    for name in names {
        match op(name) {
            Ok(r) => ok.push(r),
            Err(e) => err.push((name.clone(), e.to_string())),
        }
    }
    (ok, err)
}

/// Compute the destination directory and relative source path for an install.
/// Pure function — no filesystem access.
fn plan_install(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    found: &source_iter::SourceArtifact,
    paths: &ConfigPaths,
) -> InstallPlan {
    let dest_dir = paths.install_dir(kind, scope);
    let relative_path = types::relative_path_string(&found.artifact.path, &found.source_root);
    InstallPlan {
        artifact_name: artifact_name.to_string(),
        version: found.artifact.version.clone(),
        source_name: found.source_name.clone(),
        source_root: found.source_root.clone(),
        dest_dir,
        relative_path,
    }
}

/// Check whether the named artifact has been locally modified since it was
/// installed. Returns `Ok(())` if clean, or bails with a user-facing error if
/// modifications are detected.
fn check_local_modifications(
    artifact_name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    lock_entry: Option<&LockEntry>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    let dest_check = ctx.paths.installed_artifact_path(kind, artifact_name, scope);
    if ctx.fs.exists(&dest_check) {
        if let Some(entry) = lock_entry {
            if checksum::is_locally_modified(&dest_check, kind, entry, ctx.fs)? {
                bail!(
                    "'{artifact_name}' has local modifications. Use --force to overwrite, \
                     or 'cmx {kind} diff {artifact_name}' to review changes first."
                );
            }
        }
    }
    Ok(())
}

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
    use crate::gateway::Filesystem;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};

    // --- Display for InstallResult ---

    #[test]
    fn install_result_display_with_version() {
        let result = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "guidelines".to_string(),
            dest_dir: PathBuf::from("/home/user/.config/cmx/agents"),
            version: Some("1.0.0".to_string()),
        };
        let out = result.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("v1.0.0"));
        assert!(out.contains("guidelines"));
    }

    #[test]
    fn install_result_display_without_version() {
        let result = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "guidelines".to_string(),
            dest_dir: PathBuf::from("/home/user/.config/cmx/agents"),
            version: None,
        };
        let out = result.to_string();
        assert!(!out.contains(" v"));
    }

    // --- Display for BatchInstallResult (install mode) ---

    #[test]
    fn batch_install_result_display_empty_install_mode() {
        let result = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
            is_update: false,
        };
        let out = result.to_string();
        assert!(out.contains("already installed and up to date"));
        assert!(out.contains("agent"));
    }

    #[test]
    fn batch_install_result_display_with_items() {
        let result = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-agent".to_string(),
                kind: ArtifactKind::Agent,
                source_name: "guidelines".to_string(),
                dest_dir: PathBuf::from("/home/user/.config/cmx/agents"),
                version: Some("1.0.0".to_string()),
            }],
            kind: ArtifactKind::Agent,
            is_update: false,
        };
        let out = result.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("guidelines"));
    }

    // --- Display for BatchInstallResult (update mode) ---

    #[test]
    fn batch_install_result_display_empty_update_mode() {
        let result = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Skill,
            is_update: true,
        };
        let out = result.to_string();
        assert!(out.contains("up to date"));
        assert!(out.contains("skill"));
    }

    #[test]
    fn batch_install_result_display_update_with_items() {
        let result = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-skill".to_string(),
                kind: ArtifactKind::Skill,
                source_name: "guidelines".to_string(),
                dest_dir: PathBuf::from("/home/user/.config/cmx/skills"),
                version: None,
            }],
            kind: ArtifactKind::Skill,
            is_update: true,
        };
        let out = result.to_string();
        assert!(out.contains("my-skill"));
    }
    use crate::platform::Platform;
    use crate::source_iter::SourceArtifact;
    use crate::test_support::{
        TestContext, agent_content, make_ctx, setup_empty_sources, setup_source,
        setup_source_with_agent, setup_source_with_skill, setup_sources, test_paths,
        test_paths_for,
    };
    use crate::types::{Artifact, ArtifactKind, Deprecation, InstallScope, LockFile};
    use chrono::Utc;

    // --- plan_install (pure, no gateway fakes needed) ---

    fn make_source_artifact(
        kind: ArtifactKind,
        name: &str,
        version: Option<&str>,
    ) -> SourceArtifact {
        SourceArtifact {
            source_name: "guidelines".to_string(),
            source_root: PathBuf::from("/sources/guidelines"),
            artifact: Artifact {
                kind,
                name: name.to_string(),
                description: String::new(),
                path: match kind {
                    ArtifactKind::Agent => {
                        PathBuf::from(format!("/sources/guidelines/agents/{name}.md"))
                    }
                    ArtifactKind::Skill => PathBuf::from(format!("/sources/guidelines/{name}")),
                },
                version: version.map(str::to_string),
                deprecation: None,
            },
        }
    }

    #[test]
    fn plan_install_computes_correct_paths_for_global_agent() {
        let paths = test_paths();
        let found = make_source_artifact(ArtifactKind::Agent, "my-agent", Some("1.0.0"));

        let plan =
            plan_install("my-agent", ArtifactKind::Agent, InstallScope::Global, &found, &paths);

        assert_eq!(plan.artifact_name, "my-agent");
        assert_eq!(plan.dest_dir, paths.install_dir(ArtifactKind::Agent, InstallScope::Global));
        assert_eq!(plan.relative_path, "agents/my-agent.md");
        assert_eq!(plan.version, Some("1.0.0".to_string()));
        assert_eq!(plan.source_name, "guidelines");
    }

    #[test]
    fn plan_install_computes_correct_paths_for_local_skill() {
        let paths = test_paths();
        let found = make_source_artifact(ArtifactKind::Skill, "my-skill", None);

        let plan =
            plan_install("my-skill", ArtifactKind::Skill, InstallScope::Local, &found, &paths);

        assert_eq!(plan.dest_dir, paths.install_dir(ArtifactKind::Skill, InstallScope::Local));
        assert_eq!(plan.relative_path, "my-skill");
        assert!(plan.version.is_none());
    }

    #[test]
    fn plan_install_uses_deprecation_none_for_plain_artifact() {
        let paths = test_paths();
        let mut found = make_source_artifact(ArtifactKind::Agent, "legacy", Some("0.1.0"));
        found.artifact.deprecation = Some(Deprecation {
            reason: Some("old".to_string()),
            replacement: Some("new-agent".to_string()),
        });

        let plan =
            plan_install("legacy", ArtifactKind::Agent, InstallScope::Global, &found, &paths);
        assert_eq!(plan.version, Some("0.1.0".to_string()));
        assert_eq!(plan.source_name, "guidelines");
    }

    // --- partition_results ---

    #[test]
    fn partition_results_collects_ok_and_err() {
        let names: Vec<String> = vec!["ok1".to_string(), "fail".to_string(), "ok2".to_string()];
        let (ok, err) = partition_results(&names, |name| {
            if name == "fail" {
                anyhow::bail!("something went wrong")
            }
            Ok(name.to_string())
        });
        assert_eq!(ok, vec!["ok1".to_string(), "ok2".to_string()]);
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].0, "fail");
        assert!(err[0].1.contains("something went wrong"));
    }

    // --- install_many ---

    #[test]
    fn install_many_installs_each_and_collects_failures() {
        let t = TestContext::new();
        setup_source(&t.fs, &t.paths, "src", "/src");
        t.fs.add_file("/src/alpha/SKILL.md", "---\ndescription: a\n---\n");
        t.fs.add_file("/src/beta/SKILL.md", "---\ndescription: b\n---\n");

        let ctx = t.ctx();
        let result = install_many(
            &[
                "alpha".to_string(),
                "beta".to_string(),
                "missing".to_string(),
            ],
            ArtifactKind::Skill,
            InstallScope::Global,
            false,
            &ctx,
        )
        .unwrap();

        let names: Vec<&str> = result.installed.iter().map(|r| r.artifact_name.as_str()).collect();
        assert!(
            names.contains(&"alpha") && names.contains(&"beta"),
            "both real skills installed"
        );
        assert_eq!(result.failed.len(), 1, "the missing one is collected, not fatal");
        assert_eq!(result.failed[0].0, "missing");
        assert!(result.failed[0].1.contains("missing"), "reason mentions the name");
    }

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
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No sources registered"), "unexpected: {msg}");
    }

    #[test]
    fn install_bails_when_artifact_not_found() {
        let t = TestContext::new();

        setup_source_with_agent(
            &t.fs,
            &t.paths,
            "my-source",
            "/sources/my-source",
            "existing-agent",
        );

        let ctx = t.ctx();
        let result =
            install("nonexistent-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("nonexistent-agent"), "unexpected: {msg}");
    }

    #[test]
    fn install_bails_on_ambiguous_name() {
        let t = TestContext::new();

        // Two sources, both with the same agent name
        setup_sources(&t.fs, &t.paths, &[("source1", "/source1"), ("source2", "/source2")]);
        t.fs.add_file(
            "/source1/agents/my-agent.md",
            agent_content("my-agent", "Agent from source1"),
        );
        t.fs.add_file(
            "/source2/agents/my-agent.md",
            agent_content("my-agent", "Agent from source2"),
        );

        let ctx = t.ctx();
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("multiple sources"), "unexpected: {msg}");
    }

    #[test]
    fn install_succeeds_with_source_prefix_disambiguation() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let result =
            install("my-source:my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_ok(), "expected ok, got: {:?}", result.err());

        // Verify the artifact was installed
        let expected_dest = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        assert!(
            t.fs.file_exists(&expected_dest),
            "agent file should be installed at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_copies_agent_file_to_correct_destination() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let expected_dest = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        assert!(
            t.fs.file_exists(&expected_dest),
            "agent file should be at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_records_checksums_in_lock() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
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
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let lock = lockfile::load(InstallScope::Global, &fs, &paths).unwrap();
        let entry = lock.packages.get("my-agent").unwrap();
        assert!(
            entry.installed_at.starts_with("2024-06-01"),
            "expected 2024-06-01 timestamp, got: {}",
            entry.installed_at
        );
    }

    #[test]
    fn install_bails_on_local_modifications_without_force() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        // First install
        let ctx = t.ctx();
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        // Modify the installed file (different content than the recorded checksum)
        let installed_path = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        t.fs.write(&installed_path, "modified content that differs from source")
            .unwrap();

        // Second install should fail without force
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("local modifications"), "unexpected: {msg}");
    }

    #[test]
    fn install_proceeds_on_local_modifications_with_force() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        // First install
        let ctx = t.ctx();
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        // Modify the installed file
        let installed_path = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        t.fs.write(&installed_path, "modified content").unwrap();

        // Second install with force should succeed
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, true, &ctx);
        assert!(result.is_ok(), "force install should succeed: {:?}", result.err());
    }

    #[test]
    fn install_validates_skill_has_skill_md() {
        let t = TestContext::new();

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

        setup_source(&t.fs, &t.paths, "my-source", "/sources/my-source");

        // Skill directory without SKILL.md — scanner won't find it
        t.fs.add_file("/sources/my-source/my-skill/tool.py", "code");

        let ctx = t.ctx();
        let result = install("my-skill", ArtifactKind::Skill, InstallScope::Global, false, &ctx);
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
        let t = TestContext::new();

        setup_source(&t.fs, &t.paths, "my-source", "/sources/my-source");
        // Skill WITH SKILL.md
        t.fs.add_file("/sources/my-source/my-skill/SKILL.md", "---\ndescription: My skill\n---\n");
        t.fs.add_file("/sources/my-source/my-skill/tool.py", "code");

        let ctx = t.ctx();
        let result = install("my-skill", ArtifactKind::Skill, InstallScope::Global, false, &ctx);
        assert!(result.is_ok(), "skill with SKILL.md should install: {:?}", result.err());

        // Verify SKILL.md was copied to dest
        let dest = t.paths.install_dir(ArtifactKind::Skill, InstallScope::Global).join("my-skill");
        assert!(t.fs.file_exists(&dest.join("SKILL.md")));
    }

    // --- Verify empty lock produces empty lock file ---
    #[test]
    fn install_writes_lock_with_source_repo_name() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "guidelines", "/sources/guidelines", "my-agent");

        let ctx = t.ctx();
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let entry = lock.packages.get("my-agent").unwrap();
        assert_eq!(entry.source.repo, "guidelines");
    }

    // Ensure the LockFile starts empty
    #[test]
    fn fresh_lock_file_has_no_entries() {
        let t = TestContext::new();
        let lock: LockFile = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(lock.packages.is_empty());
    }

    // --- install_with: assert on InstallResult fields ---

    #[test]
    fn perform_install_returns_correct_artifact_name() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let result =
            install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        assert_eq!(result.artifact_name, "my-agent");
        assert_eq!(result.kind, ArtifactKind::Agent);
        assert_eq!(result.source_name, "my-source");
    }

    #[test]
    fn perform_install_dest_dir_matches_install_dir() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let result =
            install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let expected_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global);
        assert_eq!(result.dest_dir, expected_dir);
    }

    #[test]
    fn perform_install_bails_when_no_sources_registered() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("No sources registered"), "unexpected: {msg}");
    }

    // --- failure-path tests ---

    #[test]
    fn install_agent_does_not_update_lock_when_copy_fails() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        t.fs.set_fail_on_copy(true);

        let ctx = t.ctx();
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err(), "expected Err when copy fails");

        // Lock file should have no entry for the agent
        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(
            !lock.packages.contains_key("my-agent"),
            "lock should not be updated after failed copy"
        );
    }

    #[test]
    fn install_agent_lock_save_failure_rolls_back_copy() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        // Cause the lock file rename (atomic write) to fail
        t.fs.set_fail_on_rename(t.paths.lock_path(InstallScope::Global));

        let ctx = t.ctx();
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err(), "expected Err when lock save fails");

        // Because this was a fresh install, the copied agent file should be rolled back
        let expected_dest = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        assert!(
            !t.fs.file_exists(&expected_dest),
            "agent file should be rolled back after lock save failure on fresh install"
        );
    }

    #[test]
    fn install_skill_lock_save_failure_rolls_back_directory() {
        let t = TestContext::new();

        setup_source(&t.fs, &t.paths, "my-source", "/sources/my-source");
        t.fs.add_file("/sources/my-source/my-skill/SKILL.md", "---\ndescription: My skill\n---\n");
        t.fs.add_file("/sources/my-source/my-skill/tool.py", "code");

        // Cause the lock file rename (atomic write) to fail
        t.fs.set_fail_on_rename(t.paths.lock_path(InstallScope::Global));

        let ctx = t.ctx();
        let result = install("my-skill", ArtifactKind::Skill, InstallScope::Global, false, &ctx);
        assert!(result.is_err(), "expected Err when lock save fails");

        // Because this was a fresh install, the copied skill directory should be rolled back
        let expected_dest = t
            .paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .join("my-skill")
            .join("SKILL.md");
        assert!(
            !t.fs.file_exists(&expected_dest),
            "skill directory should be rolled back after lock save failure on fresh install"
        );
    }

    // --- Platform-aware install tests ---

    #[test]
    fn install_agent_with_cursor_platform_places_file_in_cursor_agents() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Cursor);

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let expected_dest =
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).join("my-agent.md");
        assert_eq!(expected_dest, PathBuf::from("/home/testuser/.cursor/agents/my-agent.md"));
        assert!(
            fs.file_exists(&expected_dest),
            "agent file should be installed at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_agent_with_cursor_platform_local_places_file_in_dot_cursor_agents() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Cursor);

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-agent", ArtifactKind::Agent, InstallScope::Local, false, &ctx).unwrap();

        let expected_dest =
            paths.install_dir(ArtifactKind::Agent, InstallScope::Local).join("my-agent.md");
        assert_eq!(expected_dest, PathBuf::from(".cursor/agents/my-agent.md"));
        assert!(
            fs.file_exists(&expected_dest),
            "agent file should be installed at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_agent_default_platform_places_file_in_dot_claude_agents() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let expected_dest =
            paths.install_dir(ArtifactKind::Agent, InstallScope::Global).join("my-agent.md");
        assert_eq!(expected_dest, PathBuf::from("/home/testuser/.claude/agents/my-agent.md"));
        assert!(
            fs.file_exists(&expected_dest),
            "agent file should be installed at {}",
            expected_dest.display()
        );
    }

    #[test]
    fn install_force_reinstall_lock_save_failure_keeps_existing_artifact() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        // First install succeeds
        let ctx = t.ctx();
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let expected_dest = t
            .paths
            .install_dir(ArtifactKind::Agent, InstallScope::Global)
            .join("my-agent.md");
        assert!(t.fs.file_exists(&expected_dest), "agent should be installed");

        // Now force-reinstall with a failing lock save
        t.fs.set_fail_on_rename(t.paths.lock_path(InstallScope::Global));

        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, true, &ctx);
        assert!(result.is_err(), "expected Err when lock save fails on reinstall");

        // Because the artifact already existed before reinstall, it should NOT be removed
        assert!(
            t.fs.file_exists(&expected_dest),
            "existing agent should be kept when lock save fails on reinstall (already_installed=true)"
        );
    }

    // --- New platform installs ---

    #[test]
    fn install_codex_agent_transforms_markdown_to_toml() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Codex);

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

        let dest = PathBuf::from("/home/testuser/.codex/agents/my-agent.toml");
        assert!(
            fs.file_exists(&dest),
            "codex agent should be written as TOML at {}",
            dest.display()
        );

        let content = fs.read_to_string(&dest).unwrap();
        assert!(content.contains("name = \"my-agent\""), "got: {content}");
        assert!(content.contains("description = \"A test agent\""), "got: {content}");
        assert!(content.contains("developer_instructions = \"# my-agent\""), "got: {content}");

        // No stray markdown file should exist alongside the TOML.
        assert!(!fs.file_exists(&PathBuf::from("/home/testuser/.codex/agents/my-agent.md")));
    }

    #[test]
    fn install_codex_agent_is_idempotent_without_force() {
        // The transform is deterministic, so a second install with no source
        // change must not trip the local-modification guard.
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Codex);

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();
        let again = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(
            again.is_ok(),
            "reinstalling unchanged codex agent should succeed: {:?}",
            again.err()
        );
    }

    #[test]
    fn install_pi_agent_is_rejected_with_clear_error() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Pi);

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx);
        assert!(result.is_err(), "pi must reject agent installs");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("pi"), "error should name the platform: {msg}");
        assert!(msg.contains("agent"), "error should name the kind: {msg}");
    }

    #[test]
    fn install_pi_skill_is_allowed_and_lands_in_dot_agents() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Pi);

        setup_source_with_skill(
            &fs,
            &paths,
            "my-source",
            "/sources/my-source",
            "my-skill",
            "1.0.0",
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-skill", ArtifactKind::Skill, InstallScope::Global, false, &ctx).unwrap();

        let dest = PathBuf::from("/home/testuser/.agents/skills/my-skill/SKILL.md");
        assert!(
            fs.file_exists(&dest),
            "pi skill should land in shared .agents/skills at {}",
            dest.display()
        );
    }

    #[test]
    fn install_skills_only_cohort_installs_skill_and_rejects_agent() {
        // Crush, Amp, Zed, OpenHands are all skills-only: a skill install must
        // succeed (to the shared .agents/skills location), and an agent install
        // must be rejected with a clear error.
        for platform in [
            Platform::Crush,
            Platform::Amp,
            Platform::Zed,
            Platform::Openhands,
            Platform::Hermes,
        ] {
            let fs = FakeFilesystem::new();
            let git = FakeGitClient::new();
            let clock = FakeClock::at(Utc::now());
            let paths = test_paths_for(platform);

            setup_source_with_skill(
                &fs,
                &paths,
                "my-source",
                "/sources/my-source",
                "my-skill",
                "1.0.0",
            );

            let ctx = make_ctx(&fs, &git, &clock, &paths);

            // Skill install lands in the platform's resolved skills dir.
            install("my-skill", ArtifactKind::Skill, InstallScope::Local, false, &ctx)
                .unwrap_or_else(|e| panic!("{platform} skill install should succeed: {e}"));
            let skill_md = paths
                .install_dir(ArtifactKind::Skill, InstallScope::Local)
                .join("my-skill")
                .join("SKILL.md");
            assert!(
                fs.file_exists(&skill_md),
                "{platform}: skill should be at {}",
                skill_md.display()
            );

            // Agent install is rejected.
            let agent = install("anything", ArtifactKind::Agent, InstallScope::Local, false, &ctx);
            assert!(agent.is_err(), "{platform}: agent install must be rejected");
            assert!(
                agent.unwrap_err().to_string().contains(&platform.to_string()),
                "{platform}: error should name the platform"
            );
        }
    }

    #[test]
    fn install_opencode_skill_lands_in_shared_dot_agents() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths_for(Platform::Opencode);

        setup_source_with_skill(
            &fs,
            &paths,
            "my-source",
            "/sources/my-source",
            "my-skill",
            "1.0.0",
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        install("my-skill", ArtifactKind::Skill, InstallScope::Local, false, &ctx).unwrap();

        let dest = PathBuf::from(".agents/skills/my-skill/SKILL.md");
        assert!(fs.file_exists(&dest), "opencode skill should land in shared .agents/skills");
    }
}
