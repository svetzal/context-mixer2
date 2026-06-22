use super::*;
use crate::gateway::Filesystem;
use crate::test_support::{TestContext, versioned_skill_content};
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

    let r =
        sync("mailctl", ArtifactKind::Skill, InstallScope::Global, None, false, &t.ctx()).unwrap();

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
        false,
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

    let err =
        sync("s", ArtifactKind::Skill, InstallScope::Global, None, false, &t.ctx()).unwrap_err();
    assert!(err.to_string().contains("--from"), "ambiguous case should ask for --from");

    sync(
        "s",
        ArtifactKind::Skill,
        InstallScope::Global,
        Some(Platform::Claude),
        false,
        &t.ctx(),
    )
    .unwrap();
    assert!(read_skill(&t, Platform::Codex, "s").contains("alpha"));
}

#[test]
fn sync_identical_copies_reports_already_synced() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "s", "1.0.0");
    place_skill(&t, Platform::Codex, "s", "1.0.0");

    let r = sync("s", ArtifactKind::Skill, InstallScope::Global, None, false, &t.ctx()).unwrap();
    assert!(r.already_synced);
    assert!(r.targets.is_empty());
}

#[test]
fn sync_dry_run_changes_nothing_on_disk() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "s", "1.1.2");
    place_skill(&t, Platform::Codex, "s", "1.0.3");

    let r = sync("s", ArtifactKind::Skill, InstallScope::Global, None, true, &t.ctx()).unwrap();
    assert!(r.dry_run);
    assert_eq!(r.targets.len(), 1);
    // Codex copy is untouched.
    assert!(read_skill(&t, Platform::Codex, "s").contains("version: 1.0.3"));
}

#[test]
fn sync_rejects_agents() {
    let t = TestContext::new();
    let err =
        sync("a", ArtifactKind::Agent, InstallScope::Global, None, false, &t.ctx()).unwrap_err();
    assert!(err.to_string().contains("skills only"));
}

#[test]
fn sync_uninstalled_skill_errors() {
    let t = TestContext::new();
    let err = sync("ghost", ArtifactKind::Skill, InstallScope::Global, None, false, &t.ctx())
        .unwrap_err();
    assert!(err.to_string().contains("not installed"));
}
