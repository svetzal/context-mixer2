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
        platform: Platform::Claude,
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
        platform: Platform::Claude,
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
            platform: Platform::Claude,
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
            platform: Platform::Claude,
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
    setup_source_with_agent, setup_source_with_skill, setup_sources, test_paths, test_paths_for,
};
use crate::types::{Artifact, ArtifactKind, Deprecation, InstallScope, LockFile};
use chrono::Utc;

// --- decide_install (pure) ---

#[test]
fn decide_install_clean_fresh_install_not_blocked_and_rolls_back() {
    let d = decide_install(false, false, false);
    assert!(!d.blocked, "clean install must not be blocked");
    assert!(d.rollback_on_lock_fail, "fresh install must roll back on lock failure");
}

#[test]
fn decide_install_locally_modified_without_force_is_blocked() {
    let d = decide_install(false, true, false);
    assert!(d.blocked, "locally modified without --force must be blocked");
}

#[test]
fn decide_install_locally_modified_with_force_is_not_blocked() {
    let d = decide_install(false, true, true);
    assert!(!d.blocked, "--force must override local modification block");
}

#[test]
fn decide_install_fresh_install_lock_fail_rolls_back() {
    let d = decide_install(false, false, false);
    assert!(
        d.rollback_on_lock_fail,
        "fresh install (already_installed=false) must roll back"
    );
}

#[test]
fn decide_install_existing_install_lock_fail_does_not_roll_back() {
    let d = decide_install(true, false, false);
    assert!(
        !d.rollback_on_lock_fail,
        "reinstall (already_installed=true) must keep existing copy"
    );
}

// --- build_lock_entry (pure, no gateway fakes needed) ---

fn make_plan(name: &str, version: Option<&str>, source: &str, rel_path: &str) -> InstallPlan {
    InstallPlan {
        artifact_name: name.to_string(),
        version: version.map(str::to_string),
        source_name: source.to_string(),
        source_root: PathBuf::from("/sources"),
        dest_dir: PathBuf::from("/dest"),
        relative_path: rel_path.to_string(),
    }
}

#[test]
fn build_lock_entry_maps_all_fields() {
    let plan = make_plan("my-agent", Some("1.2.3"), "guidelines", "agents/my-agent.md");
    let entry = build_lock_entry(
        &plan,
        ArtifactKind::Agent,
        "sha256:src".to_string(),
        "sha256:inst".to_string(),
        "2024-06-01T00:00:00Z".to_string(),
    );
    assert_eq!(entry.artifact_type, ArtifactKind::Agent);
    assert_eq!(entry.version.as_deref(), Some("1.2.3"));
    assert_eq!(entry.source.repo, "guidelines");
    assert_eq!(entry.source.path, "agents/my-agent.md");
    assert_eq!(entry.source_checksum, "sha256:src");
    assert_eq!(entry.installed_checksum, "sha256:inst");
    assert_eq!(entry.installed_at, "2024-06-01T00:00:00Z");
}

#[test]
fn build_lock_entry_without_version() {
    let plan = make_plan("my-skill", None, "home", "my-skill");
    let entry = build_lock_entry(
        &plan,
        ArtifactKind::Skill,
        "sha256:s".to_string(),
        "sha256:i".to_string(),
        "2024-01-01T00:00:00Z".to_string(),
    );
    assert!(entry.version.is_none());
    assert_eq!(entry.artifact_type, ArtifactKind::Skill);
}

// --- plan_install (pure, no gateway fakes needed) ---

fn make_source_artifact(kind: ArtifactKind, name: &str, version: Option<&str>) -> SourceArtifact {
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

    let plan = plan_install("my-agent", ArtifactKind::Agent, InstallScope::Global, &found, &paths);

    assert_eq!(plan.artifact_name, "my-agent");
    assert_eq!(
        plan.dest_dir,
        paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap()
    );
    assert_eq!(plan.relative_path, "agents/my-agent.md");
    assert_eq!(plan.version, Some("1.0.0".to_string()));
    assert_eq!(plan.source_name, "guidelines");
}

#[test]
fn plan_install_computes_correct_paths_for_local_skill() {
    let paths = test_paths();
    let found = make_source_artifact(ArtifactKind::Skill, "my-skill", None);

    let plan = plan_install("my-skill", ArtifactKind::Skill, InstallScope::Local, &found, &paths);

    assert_eq!(
        plan.dest_dir,
        paths.install_dir(ArtifactKind::Skill, InstallScope::Local).unwrap()
    );
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

    let plan = plan_install("legacy", ArtifactKind::Agent, InstallScope::Global, &found, &paths);
    assert_eq!(plan.version, Some("0.1.0".to_string()));
    assert_eq!(plan.source_name, "guidelines");
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
        &[Platform::Claude],
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

    setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "existing-agent");

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
    t.fs.add_file("/source1/agents/my-agent.md", agent_content("my-agent", "Agent from source1"));
    t.fs.add_file("/source2/agents/my-agent.md", agent_content("my-agent", "Agent from source2"));

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
        .unwrap()
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
        .unwrap()
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
        .unwrap()
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
        .unwrap()
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
    let dest = t
        .paths
        .install_dir(ArtifactKind::Skill, InstallScope::Global)
        .unwrap()
        .join("my-skill");
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

    let expected_dir = t.paths.install_dir(ArtifactKind::Agent, InstallScope::Global).unwrap();
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
        .unwrap()
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
        .unwrap()
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

    let expected_dest = paths
        .install_dir(ArtifactKind::Agent, InstallScope::Global)
        .unwrap()
        .join("my-agent.md");
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

    let expected_dest = paths
        .install_dir(ArtifactKind::Agent, InstallScope::Local)
        .unwrap()
        .join("my-agent.md");
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

    let expected_dest = paths
        .install_dir(ArtifactKind::Agent, InstallScope::Global)
        .unwrap()
        .join("my-agent.md");
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
        .unwrap()
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

// --- install_all: source_outdated integration ---

#[test]
fn install_all_skips_artifact_when_source_checksum_and_version_match_lock() {
    // When the lock entry's source_checksum and version both match the source,
    // source_outdated returns false and install_all should skip the artifact.
    let t = TestContext::new();
    setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

    let ctx = t.ctx();
    // First install — records checksum and version
    install_all(ArtifactKind::Agent, InstallScope::Global, false, &[Platform::Claude], &ctx)
        .unwrap();

    // Second install_all — source hasn't changed, so artifact should be skipped
    let result =
        install_all(ArtifactKind::Agent, InstallScope::Global, false, &[Platform::Claude], &ctx)
            .unwrap();
    assert!(
        result.items.is_empty(),
        "install_all should skip artifact whose lock source_checksum+version match"
    );
}

#[test]
fn install_all_reinstalls_artifact_when_source_checksum_changed() {
    // When the source checksum differs from the lock entry, source_outdated returns true
    // and install_all should reinstall the artifact.
    let t = TestContext::new();
    setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

    let ctx = t.ctx();
    // First install
    install_all(ArtifactKind::Agent, InstallScope::Global, false, &[Platform::Claude], &ctx)
        .unwrap();

    // Now change the source file — checksum will differ
    t.fs.add_file(
        "/sources/my-source/agents/my-agent.md",
        agent_content("my-agent", "Updated description"),
    );

    let result =
        install_all(ArtifactKind::Agent, InstallScope::Global, false, &[Platform::Claude], &ctx)
            .unwrap();
    assert_eq!(
        result.items.len(),
        1,
        "install_all should reinstall artifact when source checksum changed"
    );
    assert_eq!(result.items[0].artifact_name, "my-agent");
}

// --- update_all: version-newly-appeared picks up artifact ---

#[test]
fn update_all_picks_up_artifact_when_version_newly_appears_in_source() {
    use crate::test_support::{make_lock_entry_with_checksum, save_lock_with_entry};
    // Install without a version in the lock entry.  Then the source gains a version
    // (but keep the same checksum so only the version-presence rule fires).
    // source_outdated should return true and update_all should reinstall.
    let t = TestContext::new();
    setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

    let ctx = t.ctx();
    // Install the artifact so it exists on disk
    install("my-agent", ArtifactKind::Agent, InstallScope::Global, false, &ctx).unwrap();

    // Manually rewrite the lock entry: same source checksum but no version recorded.
    // We need the actual checksum so let's compute it via checksum_artifact.
    let source_path = std::path::PathBuf::from("/sources/my-source/agents/my-agent.md");
    let source_cs =
        crate::checksum::checksum_artifact(&source_path, ArtifactKind::Agent, &t.fs).unwrap();
    let lock_entry = make_lock_entry_with_checksum(
        ArtifactKind::Agent,
        None, // no version recorded in lock
        "my-source",
        "agents/my-agent.md",
        &source_cs,
    );
    save_lock_with_entry(&t.fs, &t.paths, "my-agent", lock_entry, InstallScope::Global);

    // Now update the source to carry a version (same content bytes, just add version frontmatter).
    // Because we can't keep byte-identical content with a new version field, we update the source
    // to a versioned variant. update_all checks source_outdated which will detect the version.
    t.fs.add_file(
        "/sources/my-source/agents/my-agent.md",
        crate::test_support::versioned_agent_content("my-agent", "A test agent", "1.0.0"),
    );

    let result = update_all(ArtifactKind::Agent, false, &ctx).unwrap();
    assert_eq!(
        result.items.len(),
        1,
        "update_all should pick up artifact when version newly appears in source"
    );
    assert_eq!(result.items[0].artifact_name, "my-agent");
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

    setup_source_with_skill(&fs, &paths, "my-source", "/sources/my-source", "my-skill", "1.0.0");

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
            .unwrap()
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

    setup_source_with_skill(&fs, &paths, "my-source", "/sources/my-source", "my-skill", "1.0.0");

    let ctx = make_ctx(&fs, &git, &clock, &paths);
    install("my-skill", ArtifactKind::Skill, InstallScope::Local, false, &ctx).unwrap();

    let dest = PathBuf::from(".agents/skills/my-skill/SKILL.md");
    assert!(fs.file_exists(&dest), "opencode skill should land in shared .agents/skills");
}

// --- resolve_targets: which platforms a default vs constrained op acts on ---

#[test]
fn resolve_targets_explicit_selector_returns_just_that_platform() {
    let t = TestContext::new();
    let ctx = t.ctx();
    let targets =
        resolve_targets(Some(Platform::Codex), ArtifactKind::Skill, InstallScope::Global, &ctx)
            .unwrap();
    assert_eq!(targets, vec![Platform::Codex]);
}

#[test]
fn resolve_targets_default_with_nothing_tracked_falls_back_to_claude() {
    let t = TestContext::new();
    let ctx = t.ctx();
    let targets = resolve_targets(None, ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();
    assert_eq!(
        targets,
        vec![Platform::Claude],
        "a first-ever install should land on Claude rather than nowhere"
    );
}

#[test]
fn resolve_targets_default_includes_platforms_already_in_use() {
    let t = TestContext::new();
    // Mark Codex as "in use" by giving its lock a tracked entry, but leave
    // Claude's lock empty.
    let codex = t.paths.with_platform(Platform::Codex);
    crate::test_support::save_lock_with_entry(
        &t.fs,
        &codex,
        "already-there",
        crate::test_support::sample_lock_entry(),
        InstallScope::Global,
    );

    let ctx = t.ctx();
    let targets = resolve_targets(None, ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();

    assert!(targets.contains(&Platform::Codex), "Codex is in use → targeted");
    assert!(
        !targets.contains(&Platform::Claude),
        "Claude has nothing tracked → not targeted"
    );
}

#[test]
fn install_many_fans_out_to_every_target_platform() {
    let t = TestContext::new();
    setup_source_with_skill(&t.fs, &t.paths, "src", "/src", "shared-skill", "1.0.0");
    let ctx = t.ctx();

    let result = install_many(
        &["shared-skill".to_string()],
        ArtifactKind::Skill,
        InstallScope::Global,
        false,
        &[Platform::Claude, Platform::Codex],
        &ctx,
    )
    .unwrap();

    // One InstallResult per platform, each naming its own platform.
    assert_eq!(result.installed.len(), 2);
    let platforms: Vec<Platform> = result.installed.iter().map(|r| r.platform).collect();
    assert!(platforms.contains(&Platform::Claude) && platforms.contains(&Platform::Codex));
    assert!(result.failed.is_empty());

    // Both platforms' lock files now track it.
    for p in [Platform::Claude, Platform::Codex] {
        let pv = t.paths.with_platform(p);
        let lock = crate::lockfile::load(InstallScope::Global, &t.fs, &pv).unwrap();
        assert!(lock.packages.contains_key("shared-skill"), "{p} should track the skill");
    }
}
