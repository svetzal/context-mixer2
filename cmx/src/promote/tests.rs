use super::*;
use crate::gateway::Filesystem;
use crate::test_support::{TestContext, make_lock_entry_builder, save_lock_with_entry};

/// Place a skill copy for `platform` at its global install dir with the given
/// SKILL.md body. Returns the skill directory path.
fn place_skill(t: &TestContext, platform: Platform, name: &str, body: &str) -> std::path::PathBuf {
    let pv = t.paths.with_platform(platform);
    let dir = pv.install_dir(ArtifactKind::Skill, InstallScope::Global).unwrap();
    let skill_dir = dir.join(name);
    t.fs.add_file(skill_dir.join("SKILL.md"), format!("---\ndescription: {body}\n---\n# {body}\n"));
    skill_dir
}

/// Record a `home`-provenance lock entry for `platform` with a stale checksum,
/// so the installed copy reads as drifted from the home baseline.
fn track_from_home(t: &TestContext, platform: Platform, name: &str) {
    let mut entry = make_lock_entry_builder(ArtifactKind::Skill, HOME_SOURCE, "skills/x/SKILL.md");
    entry.installed_checksum = "sha256:stale".to_string();
    entry.source_checksum = "sha256:stale".to_string();
    save_lock_with_entry(
        &t.fs,
        &t.paths.with_platform(platform),
        name,
        entry,
        InstallScope::Global,
    );
}

fn home_skill_md(t: &TestContext, name: &str) -> std::path::PathBuf {
    t.paths.config_dir.join("home").join("skills").join(name).join("SKILL.md")
}

fn lock_entry(t: &TestContext, platform: Platform, name: &str) -> crate::types::LockEntry {
    let pv = t.paths.with_platform(platform);
    crate::lockfile::load(InstallScope::Global, &t.fs, &pv)
        .unwrap()
        .packages
        .get(name)
        .cloned()
        .expect("lock entry present")
}

// --- happy path ---

#[test]
fn promote_copies_installed_into_home_and_refreshes_lock() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "pf", "edited in place");
    track_from_home(&t, Platform::Claude, "pf");

    let r = promote("pf", ArtifactKind::Skill, None, &t.ctx()).unwrap();

    assert!(!r.already_current);
    assert_eq!(r.retracked, vec![Platform::Claude]);
    assert!(r.still_divergent.is_empty());

    // The home now holds the edited content.
    let home_md = home_skill_md(&t, "pf");
    assert!(t.fs.exists(&home_md), "home copy written");
    assert!(t.fs.read_to_string(&home_md).unwrap().contains("edited in place"));

    // The lock baseline now matches the installed copy (drift cleared).
    let pv = t.paths.with_platform(Platform::Claude);
    let installed = pv
        .installed_artifact_path(ArtifactKind::Skill, "pf", InstallScope::Global)
        .unwrap();
    let cs = crate::checksum::checksum_artifact(&installed, ArtifactKind::Skill, &t.fs).unwrap();
    let entry = lock_entry(&t, Platform::Claude, "pf");
    assert_eq!(entry.installed_checksum, cs, "installed baseline refreshed");
    assert_eq!(entry.source_checksum, cs, "source baseline refreshed");
    assert_eq!(entry.source.repo, HOME_SOURCE, "still home-provenance");
}

#[test]
fn promote_is_a_noop_when_home_already_matches() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "pf", "same content");
    track_from_home(&t, Platform::Claude, "pf");
    // Pre-seed the home with identical content.
    promote("pf", ArtifactKind::Skill, None, &t.ctx()).unwrap();

    // A second promote finds the home already current.
    let r = promote("pf", ArtifactKind::Skill, None, &t.ctx()).unwrap();
    assert!(r.already_current, "home already matches installed");
    assert!(r.retracked.is_empty());
}

#[test]
fn promote_flags_other_platforms_that_still_differ() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "pf", "claude edits");
    place_skill(&t, Platform::Codex, "pf", "different codex edits");
    track_from_home(&t, Platform::Claude, "pf");
    track_from_home(&t, Platform::Codex, "pf");

    // Explicitly promote the Claude copy; Codex differs from it and is flagged.
    let r = promote("pf", ArtifactKind::Skill, Some(Platform::Claude), &t.ctx()).unwrap();

    assert!(r.retracked.contains(&Platform::Claude));
    assert!(r.retracked.contains(&Platform::Codex));
    assert_eq!(
        r.still_divergent,
        vec![Platform::Codex],
        "Codex still differs from the promoted copy"
    );
    assert!(
        t.fs.read_to_string(&home_skill_md(&t, "pf")).unwrap().contains("claude edits"),
        "the selected (claude) copy became canonical"
    );
}

// --- drift-aware default selection ---

#[test]
fn promote_auto_selects_the_single_drifted_copy() {
    let t = TestContext::new();
    // Claude was edited in place (drifted); Codex still matches its baseline.
    place_skill(&t, Platform::Claude, "pf", "claude edits");
    place_skill(&t, Platform::Codex, "pf", "pristine");
    track_from_home(&t, Platform::Claude, "pf");
    let mut entry = make_lock_entry_builder(ArtifactKind::Skill, HOME_SOURCE, "skills/x/SKILL.md");
    // Codex's baseline matches its on-disk content → not drifted.
    let pv = t.paths.with_platform(Platform::Codex);
    let codex_path = pv
        .installed_artifact_path(ArtifactKind::Skill, "pf", InstallScope::Global)
        .unwrap();
    let codex_cs =
        crate::checksum::checksum_artifact(&codex_path, ArtifactKind::Skill, &t.fs).unwrap();
    entry.installed_checksum = codex_cs.clone();
    entry.source_checksum = codex_cs;
    save_lock_with_entry(&t.fs, &pv, "pf", entry, InstallScope::Global);

    // No --from: cmx must pick the drifted Claude copy, not default-to-Claude
    // by luck — Codex being pristine is what makes the choice unambiguous.
    let r = promote("pf", ArtifactKind::Skill, None, &t.ctx()).unwrap();
    assert!(!r.already_current);
    assert!(
        t.fs.read_to_string(&home_skill_md(&t, "pf")).unwrap().contains("claude edits"),
        "the drifted copy was promoted"
    );
}

#[test]
fn promote_refuses_when_multiple_platforms_drift_differently() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "pf", "claude edits");
    place_skill(&t, Platform::Codex, "pf", "different codex edits");
    track_from_home(&t, Platform::Claude, "pf");
    track_from_home(&t, Platform::Codex, "pf");

    // Both copies were edited in place, differently → cmx can't guess.
    let err = promote("pf", ArtifactKind::Skill, None, &t.ctx()).unwrap_err().to_string();
    assert!(err.contains("Multiple platforms"), "explains the ambiguity: {err}");
    assert!(err.contains("diff pf"), "points at diff to inspect: {err}");
    assert!(err.contains("--from"), "offers the tie-breaker flag: {err}");
    // The home is untouched — nothing was promoted.
    assert!(!t.fs.exists(&home_skill_md(&t, "pf")), "home not written on refusal");
}

// --- guard rails ---

#[test]
fn promote_rejects_git_sourced_artifact_with_guidance() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "slidev", "edited");
    let entry = make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "slidev/SKILL.md");
    save_lock_with_entry(&t.fs, &t.paths, "slidev", entry, InstallScope::Global);

    let err = promote("slidev", ArtifactKind::Skill, None, &t.ctx()).unwrap_err().to_string();
    assert!(err.contains("'guidelines' source"), "names the git source: {err}");
    assert!(err.contains("update slidev --force"), "offers the discard path: {err}");
}

#[test]
fn promote_rejects_untracked_artifact_with_guidance() {
    let t = TestContext::new();
    place_skill(&t, Platform::Claude, "loose", "hand authored");

    let err = promote("loose", ArtifactKind::Skill, None, &t.ctx()).unwrap_err().to_string();
    assert!(err.contains("adopt loose"), "steers a hand-authored artifact to adopt: {err}");
}

#[test]
fn promote_errors_when_not_installed() {
    let t = TestContext::new();
    let err = promote("ghost", ArtifactKind::Skill, None, &t.ctx()).unwrap_err().to_string();
    assert!(err.contains("No installed skill named 'ghost'"), "got: {err}");
}
