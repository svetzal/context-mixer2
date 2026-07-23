use super::*;
use crate::flags::RunMode;
use crate::gateway::Filesystem;
use crate::test_support::{
    TestContext, make_lock_entry_builder, save_lock_with_entry, versioned_skill_content,
};
use std::cmp::Ordering;

/// Place a skill copy for `platform` at its install dir, with the given version.
fn place_skill(
    t: &TestContext,
    platform: Platform,
    name: &str,
    version: &str,
) -> std::path::PathBuf {
    let pv = t.paths.with_platform(platform);
    let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
    let skill_dir = dir.join(name);
    t.fs.add_file(skill_dir.join("SKILL.md"), versioned_skill_content("a skill", version));
    skill_dir
}

/// Place an unversioned skill copy with arbitrary body (to force a content diff).
fn place_unversioned(t: &TestContext, platform: Platform, name: &str, body: &str) {
    let pv = t.paths.with_platform(platform);
    let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
    t.fs.add_file(
        dir.join(name).join("SKILL.md"),
        format!("---\ndescription: {body}\n---\n# skill {body}\n"),
    );
}

fn read_skill(t: &TestContext, platform: Platform, name: &str) -> String {
    let pv = t.paths.with_platform(platform);
    let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
    t.fs.read_to_string(&dir.join(name).join("SKILL.md")).unwrap()
}

/// Declare the managed-platform set in config (the authoritative set that scopes
/// `--from` suggestions).
fn set_managed(t: &TestContext, platforms: &[Platform]) {
    let config = crate::types::CmxConfig {
        platforms: platforms.to_vec(),
        ..Default::default()
    };
    crate::config::save_config(&config, &t.fs, &t.paths).unwrap();
}

fn set_external_rules(t: &TestContext, rules: &[&str]) {
    let config = crate::types::CmxConfig {
        external: rules.iter().map(|rule| (*rule).to_string()).collect(),
        ..Default::default()
    };
    crate::config::save_config(&config, &t.fs, &t.paths).unwrap();
}

// --- cmp_versions (pure) ---

#[test]
fn cmp_versions_orders_numerically_and_handles_absent() {
    assert_eq!(cmp_versions(Some("1.1.2"), Some("1.0.3")), Ordering::Greater);
    assert_eq!(cmp_versions(Some("1.0.3"), Some("1.1.2")), Ordering::Less);
    assert_eq!(cmp_versions(Some("2.0.0"), Some("2.0.0")), Ordering::Equal);
    // 10 > 9 numerically, not lexically.
    assert_eq!(cmp_versions(Some("1.10.0"), Some("1.9.0")), Ordering::Greater);
    assert_eq!(cmp_versions(None, Some("0.0.1")), Ordering::Less);
    assert_eq!(cmp_versions(Some("0.0.1"), None), Ordering::Greater);
    assert_eq!(cmp_versions(None, None), Ordering::Equal);
}

// --- sync behaviour ---

#[test]
fn sync_newest_version_wins_and_updates_the_older_copy() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "mailctl", "1.1.2");
    place_skill(&t, Platform::Codex, "mailctl", "1.0.3");

    let r = sync(
        "mailctl",
        ArtifactKind::Skill,
        InstallScope::Global,
        None,
        RunMode::Apply,
        &t.ctx(),
    )
    .unwrap();

    assert!(!r.already_synced);
    assert_eq!(r.winner_version.as_deref(), Some("1.1.2"));
    assert_eq!(r.targets.len(), 1, "only the Codex copy needed updating");
    // Codex copy now carries the winner's 1.1.2 content.
    assert!(read_skill(&t, Platform::Codex, "mailctl").contains("version: 1.1.2"));
}

#[test]
fn sync_from_forces_direction_even_against_newest() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "s", "1.0.0");
    place_skill(&t, Platform::Codex, "s", "2.0.0");

    sync(
        "s",
        ArtifactKind::Skill,
        InstallScope::Global,
        Some(Platform::Claude),
        RunMode::Apply,
        &t.ctx(),
    )
    .unwrap();

    // --from claude wins despite Codex being newer.
    assert!(read_skill(&t, Platform::Codex, "s").contains("version: 1.0.0"));
}

#[test]
fn sync_ambiguous_without_from_errors_but_succeeds_with_from() {
    let t = TestContext::new();
    place_unversioned(&t, Platform::Claude, "s", "alpha");
    place_unversioned(&t, Platform::Codex, "s", "beta");

    let err = sync("s", ArtifactKind::Skill, InstallScope::Global, None, RunMode::Plan, &t.ctx())
        .unwrap_err();
    assert!(err.to_string().contains("--from"), "ambiguous case should ask for --from");

    sync(
        "s",
        ArtifactKind::Skill,
        InstallScope::Global,
        Some(Platform::Claude),
        RunMode::Apply,
        &t.ctx(),
    )
    .unwrap();
    assert!(read_skill(&t, Platform::Codex, "s").contains("alpha"));
}

#[test]
fn sync_ambiguous_error_lists_candidates_and_from_commands() {
    let t = TestContext::new();
    place_unversioned(&t, Platform::Claude, "s", "alpha");
    place_unversioned(&t, Platform::Codex, "s", "beta");
    // Managed set scopes the --from suggestion to a tool the user uses: the
    // Codex copy lives in the shared .agents/skills cohort, so without this the
    // suggestion would name some other cohort member (e.g. opencode).
    set_managed(&t, &[Platform::Claude, Platform::Codex]);

    let err = sync("s", ArtifactKind::Skill, InstallScope::Global, None, RunMode::Plan, &t.ctx())
        .unwrap_err()
        .to_string();

    // Names each candidate platform and the exact per-copy --from command.
    assert!(err.contains("claude"), "lists claude copy: {err}");
    assert!(err.contains("codex"), "lists codex copy: {err}");
    assert!(err.contains("cmx skill sync s --from claude"), "claude command: {err}");
    assert!(err.contains("cmx skill sync s --from codex"), "codex command: {err}");
}

#[test]
fn sync_ambiguous_error_suggests_promote_for_home_tracked() {
    let t = TestContext::new();
    place_unversioned(&t, Platform::Claude, "s", "alpha");
    place_unversioned(&t, Platform::Codex, "s", "beta");
    // Mark the Claude copy as tracked from the home.
    let entry = make_lock_entry_builder(ArtifactKind::Skill, "home", "skills/s/SKILL.md");
    save_lock_with_entry(
        &t.fs,
        &t.paths.with_platform(Platform::Claude),
        "s",
        entry,
        InstallScope::Global,
    );

    let err = sync("s", ArtifactKind::Skill, InstallScope::Global, None, RunMode::Plan, &t.ctx())
        .unwrap_err()
        .to_string();

    assert!(err.contains("tracked from the home"), "mentions the home path: {err}");
    assert!(err.contains("cmx skill promote s"), "offers promote: {err}");
}

#[test]
fn sync_home_tracked_copy_matching_external_rule_avoids_other_tool_claim() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "mailctl", "1.1.2");
    place_skill(&t, Platform::Codex, "mailctl", "1.0.3");
    set_external_rules(&t, &["~/.claude/skills"]);

    let entry = make_lock_entry_builder(ArtifactKind::Skill, "home", "skills/mailctl/SKILL.md");
    save_lock_with_entry(
        &t.fs,
        &t.paths.with_platform(Platform::Claude),
        "mailctl",
        entry,
        InstallScope::Global,
    );

    let out = sync(
        "mailctl",
        ArtifactKind::Skill,
        InstallScope::Global,
        None,
        RunMode::Plan,
        &t.ctx(),
    )
    .unwrap()
    .to_string();

    assert!(out.contains("matches an external rule"), "got: {out}");
    assert!(!out.contains("managed by another tool"), "got: {out}");
}

#[test]
fn sync_identical_copies_reports_already_synced() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "s", "1.0.0");
    place_skill(&t, Platform::Codex, "s", "1.0.0");

    let r = sync("s", ArtifactKind::Skill, InstallScope::Global, None, RunMode::Plan, &t.ctx())
        .unwrap();
    assert!(r.already_synced);
    assert!(r.targets.is_empty());
}

#[test]
fn sync_dry_run_changes_nothing_on_disk() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "s", "1.1.2");
    place_skill(&t, Platform::Codex, "s", "1.0.3");

    let r = sync("s", ArtifactKind::Skill, InstallScope::Global, None, RunMode::Plan, &t.ctx())
        .unwrap();
    assert!(!r.apply);
    assert_eq!(r.targets.len(), 1);
    // Codex copy is untouched.
    assert!(read_skill(&t, Platform::Codex, "s").contains("version: 1.0.3"));
}

#[test]
fn sync_rejects_agents() {
    let t = TestContext::new();
    let err = sync("a", ArtifactKind::Agent, InstallScope::Global, None, RunMode::Plan, &t.ctx())
        .unwrap_err();
    assert!(err.to_string().contains("skills only"));
}

#[test]
fn sync_uninstalled_skill_errors() {
    let t = TestContext::new();
    let err = sync(
        "ghost",
        ArtifactKind::Skill,
        InstallScope::Global,
        None,
        RunMode::Plan,
        &t.ctx(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("not installed"));
}
