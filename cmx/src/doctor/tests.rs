use std::collections::HashMap;

use super::*;
use crate::platform::Platform;
use crate::test_support::{
    TestContext, make_lock_entry_with_checksum, save_lock_with_entry, versioned_skill_content,
};
use crate::types::{ArtifactKind, InstallScope, LockFile};

/// Install a skill directory on disk for the given platform/scope and return
/// its checksum so a lock entry can be made to match (or deliberately not).
fn install_skill(
    t: &TestContext,
    platform: Platform,
    skill: &str,
    version: &str,
    scope: InstallScope,
) -> std::path::PathBuf {
    let pv = t.paths.with_platform(platform);
    let dir = pv.install_dir(ArtifactKind::Skill, scope).unwrap();
    let skill_dir = dir.join(skill);
    t.fs.add_file(skill_dir.join("SKILL.md"), versioned_skill_content("A test skill", version));
    skill_dir
}

fn skill_checksum(t: &TestContext, skill_dir: &std::path::Path) -> String {
    crate::checksum::checksum_dir(skill_dir, &t.fs).unwrap()
}

/// Install a skill for `platform` and record a matching lock entry in that
/// platform's lock file, so the survey classifies it `tracked` for that tool.
fn track_skill(t: &TestContext, platform: Platform, skill: &str, version: &str) {
    let dir = install_skill(t, platform, skill, version, InstallScope::Global);
    let cs = skill_checksum(t, &dir);
    let entry =
        make_lock_entry_with_checksum(ArtifactKind::Skill, Some(version), "home", skill, &cs);
    let pv = t.paths.with_platform(platform);
    crate::lockfile::mutate(InstallScope::Global, &t.fs, &pv, |l| {
        l.packages.insert(skill.to_string(), entry);
    })
    .unwrap();
}

// --- is_problem ---

#[test]
fn is_problem_matrix() {
    let art = |state, diverged| DoctorArtifact {
        kind: ArtifactKind::Skill,
        name: "x".to_string(),
        scope: InstallScope::Global,
        state,
        version: None,
        versions: vec![],
        tools: vec![],
        source: None,
        locations: vec![],
        diverged,
    };
    assert!(
        !DoctorReport::is_problem(&art(ArtifactState::Tracked, false)),
        "clean tracked: ok"
    );
    assert!(
        DoctorReport::is_problem(&art(ArtifactState::Tracked, true)),
        "tracked+diverged: problem"
    );
    assert!(
        DoctorReport::is_problem(&art(ArtifactState::Orphaned, false)),
        "orphaned: problem"
    );
    assert!(
        DoctorReport::is_problem(&art(ArtifactState::Untracked, false)),
        "untracked: problem"
    );
    assert!(
        DoctorReport::is_problem(&art(ArtifactState::Drifted, false)),
        "drifted: problem"
    );
    // A consistent external artifact is fine; a diverged one is an anomaly
    // worth surfacing even though its owning tool (not cmx) must re-sync it.
    assert!(
        !DoctorReport::is_problem(&art(ArtifactState::External, false)),
        "consistent external: ok"
    );
    assert!(
        DoctorReport::is_problem(&art(ArtifactState::External, true)),
        "external+diverged: surfaced as a problem"
    );
}

// --- ArtifactState::label ---

#[test]
fn artifact_state_labels() {
    assert_eq!(ArtifactState::Tracked.label(), "tracked");
    assert_eq!(ArtifactState::Drifted.label(), "drifted");
    assert_eq!(ArtifactState::Orphaned.label(), "orphaned");
}

// --- counts across mixed states ---

#[test]
fn counts_tally_tracked_and_drifted() {
    let t = TestContext::new();
    // One tracked (checksum matches lock), one drifted (lock checksum stale).
    let tracked_dir = install_skill(&t, Platform::Claude, "ok", "1.0.0", InstallScope::Global);
    let cs = skill_checksum(&t, &tracked_dir);
    install_skill(&t, Platform::Claude, "edited", "1.0.0", InstallScope::Global);
    // Both entries in one lock: "ok" matches its on-disk checksum, "edited" does not.
    crate::lockfile::mutate(InstallScope::Global, &t.fs, &t.paths, |lock| {
        lock.packages.insert(
            "ok".to_string(),
            make_lock_entry_with_checksum(ArtifactKind::Skill, Some("1.0.0"), "home", "ok", &cs),
        );
        lock.packages.insert(
            "edited".to_string(),
            make_lock_entry_with_checksum(
                ArtifactKind::Skill,
                Some("1.0.0"),
                "home",
                "edited",
                "sha256:stale",
            ),
        );
    })
    .unwrap();

    let report = survey(false, &t.ctx()).unwrap();
    let c = report.counts();
    assert_eq!(c.tracked, 1, "one tracked");
    assert_eq!(c.drifted, 1, "one drifted");
    assert_eq!(c.orphaned, 0);
}

// --- survey_scopes ---

#[test]
fn survey_scopes_global_only_by_default() {
    assert_eq!(survey_scopes(false), vec![InstallScope::Global]);
}

#[test]
fn survey_scopes_includes_local_when_requested() {
    assert_eq!(survey_scopes(true), vec![InstallScope::Global, InstallScope::Local]);
}

// --- build_locations ---

#[test]
fn build_locations_collapses_shared_agents_skills_cohort() {
    let t = TestContext::new();
    let ctx = t.ctx();
    let locations = build_locations(&ctx, &[InstallScope::Global]);

    // The shared global .agents/skills directory must be a single location
    // attributed to every cohort platform.
    let shared = t.paths.home_dir.join(".agents").join("skills");
    let agg = locations.get(&shared).expect("shared .agents/skills location present");
    assert_eq!(agg.kind, ArtifactKind::Skill);
    for p in [
        Platform::Opencode,
        Platform::Codex,
        Platform::Pi,
        Platform::Crush,
        Platform::Zed,
        Platform::Openhands,
    ] {
        assert!(agg.platforms.contains(&p), "{p} should read shared .agents/skills");
    }
}

// --- end-to-end survey classification ---

#[test]
fn orphaned_skill_in_claude_dir_is_reported() {
    let t = TestContext::new();
    // A hand-authored skill in ~/.claude/skills with no lock entry anywhere.
    install_skill(&t, Platform::Claude, "my-skill", "1.0.0", InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let row = report.rows.iter().find(|r| r.name == "my-skill").expect("skill surveyed");
    assert_eq!(row.state, ArtifactState::Orphaned);
    assert_eq!(row.version.as_deref(), Some("1.0.0"));
    assert!(report.has_issues(), "an orphan is an issue");
}

#[test]
fn untracked_when_on_disk_no_lock_but_source_provides_it() {
    let t = TestContext::new();
    // A registered source provides "vis-theory"...
    crate::test_support::setup_source_with_skill(
        &t.fs,
        &t.paths,
        "guidelines",
        "/sources/guidelines",
        "vis-theory",
        "1.0.0",
    );
    // ...and it's on disk with no lock entry (installed out-of-band).
    install_skill(&t, Platform::Claude, "vis-theory", "1.0.0", InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let row = report.rows.iter().find(|r| r.name == "vis-theory").expect("surveyed");
    assert_eq!(
        row.state,
        ArtifactState::Untracked,
        "source-available + no lock → untracked, not orphaned"
    );
    assert_eq!(report.counts().untracked, 1);
    assert_eq!(report.counts().orphaned, 0);
    assert!(report.has_issues());
}

#[test]
fn external_reclassifies_orphan_by_directory_rule() {
    let t = TestContext::new();
    crate::test_support::setup_empty_sources(&t.fs, &t.paths);
    // A stock skill from another tool, in the Claude skills dir.
    install_skill(&t, Platform::Claude, "stock-skill", "1.0.0", InstallScope::Global);
    // Declare that whole directory external (home_dir is /home/testuser).
    let cfg = crate::types::CmxConfig {
        external: vec!["~/.claude/skills".to_string()],
        ..Default::default()
    };
    crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

    let report = survey(false, &t.ctx()).unwrap();
    let row = report.rows.iter().find(|r| r.name == "stock-skill").expect("surveyed");
    assert_eq!(row.state, ArtifactState::External);
    assert_eq!(report.counts().external, 1);
    assert_eq!(report.counts().orphaned, 0);
    assert!(!report.has_issues(), "external artifacts are not issues");
}

#[test]
fn external_reclassifies_orphan_by_name_rule() {
    let t = TestContext::new();
    crate::test_support::setup_empty_sources(&t.fs, &t.paths);
    install_skill(&t, Platform::Claude, "apple", "1.0.0", InstallScope::Global);
    install_skill(&t, Platform::Claude, "mine", "1.0.0", InstallScope::Global);
    let cfg = crate::types::CmxConfig {
        external: vec!["apple".to_string()], // bare name
        ..Default::default()
    };
    crate::config::save_config(&cfg, &t.fs, &t.paths).unwrap();

    let report = survey(false, &t.ctx()).unwrap();
    assert_eq!(
        report.rows.iter().find(|r| r.name == "apple").unwrap().state,
        ArtifactState::External
    );
    assert_eq!(
        report.rows.iter().find(|r| r.name == "mine").unwrap().state,
        ArtifactState::Orphaned,
        "a non-matching orphan stays orphaned"
    );
}

#[test]
fn orphaned_only_when_no_source_provides_it() {
    let t = TestContext::new();
    // No source registered; a hand-authored skill on disk with no lock.
    crate::test_support::setup_empty_sources(&t.fs, &t.paths);
    install_skill(&t, Platform::Claude, "my-private", "1.0.0", InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let row = report.rows.iter().find(|r| r.name == "my-private").expect("surveyed");
    assert_eq!(row.state, ArtifactState::Orphaned);
    assert_eq!(report.counts().untracked, 0);
    assert_eq!(report.counts().orphaned, 1);
}

#[test]
fn tracked_artifact_reports_its_lock_source() {
    let t = TestContext::new();
    // track_skill records provenance repo "home" in the lock entry.
    track_skill(&t, Platform::Claude, "mine", "1.0.0");

    let report = survey(false, &t.ctx()).unwrap();
    let art = report.artifacts.iter().find(|a| a.name == "mine").expect("grouped");
    assert_eq!(art.source.as_deref(), Some("home"), "source from the lock entry");
}

#[test]
fn orphan_has_no_source() {
    let t = TestContext::new();
    crate::test_support::setup_empty_sources(&t.fs, &t.paths);
    install_skill(&t, Platform::Claude, "loose", "1.0.0", InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let art = report.artifacts.iter().find(|a| a.name == "loose").expect("grouped");
    assert!(art.source.is_none(), "an orphan has no source");
}

#[test]
fn tracked_skill_matches_lock_checksum() {
    let t = TestContext::new();
    let skill_dir = install_skill(&t, Platform::Claude, "tracked", "1.0.0", InstallScope::Global);
    let cs = skill_checksum(&t, &skill_dir);
    let entry =
        make_lock_entry_with_checksum(ArtifactKind::Skill, Some("1.0.0"), "home", "tracked", &cs);
    save_lock_with_entry(&t.fs, &t.paths, "tracked", entry, InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let row = report.rows.iter().find(|r| r.name == "tracked").expect("skill surveyed");
    assert_eq!(row.state, ArtifactState::Tracked);
    assert!(!report.has_issues(), "a tracked artifact is not an issue");
}

#[test]
fn drifted_skill_has_lock_entry_but_mismatched_checksum() {
    let t = TestContext::new();
    install_skill(&t, Platform::Claude, "drifted", "1.0.0", InstallScope::Global);
    let entry = make_lock_entry_with_checksum(
        ArtifactKind::Skill,
        Some("1.0.0"),
        "home",
        "drifted",
        "sha256:stale_checksum_from_install_time",
    );
    save_lock_with_entry(&t.fs, &t.paths, "drifted", entry, InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let row = report.rows.iter().find(|r| r.name == "drifted").expect("skill surveyed");
    assert_eq!(row.state, ArtifactState::Drifted);
    assert!(report.has_issues());
}

#[test]
fn missing_skill_in_lock_but_not_on_disk() {
    let t = TestContext::new();
    let entry = make_lock_entry_with_checksum(
        ArtifactKind::Skill,
        Some("1.0.0"),
        "home",
        "ghost",
        "sha256:whatever",
    );
    save_lock_with_entry(&t.fs, &t.paths, "ghost", entry, InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    assert!(report.rows.is_empty(), "nothing on disk");
    let m = report
        .missing
        .iter()
        .find(|m| m.name == "ghost")
        .expect("missing entry reported");
    assert_eq!(m.kind, ArtifactKind::Skill);
    assert_eq!(m.platform, Platform::Claude);
    assert!(report.has_issues());
}

#[test]
fn same_skill_in_two_tools_is_one_artifact_not_duplicated() {
    let t = TestContext::new();
    // Same skill, same version, tracked for claude (~/.claude/skills) and pi
    // (~/.agents/skills) — one logical artifact managed for both tools.
    track_skill(&t, Platform::Claude, "multi", "1.0.0");
    track_skill(&t, Platform::Pi, "multi", "1.0.0");

    let report = survey(false, &t.ctx()).unwrap();
    let arts: Vec<&DoctorArtifact> =
        report.artifacts.iter().filter(|a| a.name == "multi").collect();
    assert_eq!(arts.len(), 1, "one logical artifact, not two duplicates");
    assert_eq!(arts[0].state, ArtifactState::Tracked);
    assert!(!arts[0].diverged, "identical copies do not diverge");
    // Tools = the platforms cmx tracks it for (lockfile-backed), not every
    // cohort tool that merely reads .agents/skills.
    assert!(arts[0].tools.contains(&Platform::Claude));
    assert!(arts[0].tools.contains(&Platform::Pi));
    assert!(
        !arts[0].tools.contains(&Platform::Crush),
        "crush reads .agents/skills but isn't tracked for it — must not be listed"
    );
    // The raw per-location rows still exist (two locations) for adopt/detail.
    assert_eq!(report.rows.iter().filter(|r| r.name == "multi").count(), 2);
}

#[test]
fn same_skill_at_different_versions_is_diverged() {
    let t = TestContext::new();
    install_skill(&t, Platform::Claude, "skew", "1.0.0", InstallScope::Global);
    install_skill(&t, Platform::Pi, "skew", "2.0.0", InstallScope::Global);

    let report = survey(false, &t.ctx()).unwrap();
    let art = report.artifacts.iter().find(|a| a.name == "skew").expect("grouped");
    assert!(art.diverged, "different versions across locations should diverge");
    assert!(art.version.is_none(), "no single agreed version");
    assert_eq!(report.counts().diverged, 1);
    assert!(report.has_issues(), "divergence is an issue");
}

#[test]
fn shared_cohort_skill_lists_only_tools_it_is_tracked_for() {
    let t = TestContext::new();
    // One skill in the shared ~/.agents/skills dir, tracked for pi and codex
    // (both wrote lock entries). It's one artifact whose Tools lists exactly
    // those two — not the other cohort tools that merely read the directory.
    track_skill(&t, Platform::Pi, "shared", "1.0.0");
    track_skill(&t, Platform::Codex, "shared", "1.0.0");

    let report = survey(false, &t.ctx()).unwrap();
    let arts: Vec<&DoctorArtifact> =
        report.artifacts.iter().filter(|a| a.name == "shared").collect();
    assert_eq!(arts.len(), 1, "shared dir reported once");
    assert!(!arts[0].diverged, "consistent copies don't diverge");
    assert!(arts[0].tools.contains(&Platform::Pi));
    assert!(arts[0].tools.contains(&Platform::Codex));
    assert!(
        !arts[0].tools.contains(&Platform::Crush) && !arts[0].tools.contains(&Platform::Zed),
        "cohort readers without a lock entry are not listed as tracked-for tools"
    );
}

// --- divergence_details (pure, no gateway fakes needed) ---

fn make_doctor_artifact(name: &str, diverged: bool) -> DoctorArtifact {
    DoctorArtifact {
        kind: ArtifactKind::Skill,
        name: name.to_string(),
        scope: InstallScope::Global,
        state: ArtifactState::External,
        version: None,
        versions: vec![],
        tools: vec![],
        source: None,
        locations: vec![],
        diverged,
    }
}

fn make_doctor_row(name: &str, loc: &str, ver: &str, state: ArtifactState) -> DoctorRow {
    DoctorRow {
        kind: ArtifactKind::Skill,
        name: name.to_string(),
        scope: InstallScope::Global,
        location: std::path::PathBuf::from(loc),
        platforms: vec![Platform::Claude],
        tracked_for: vec![],
        state,
        version: Some(ver.to_string()),
        source: None,
    }
}

#[test]
fn divergence_details_empty_when_no_diverged_artifacts() {
    let art = make_doctor_artifact("clean", false);
    let shown = vec![&art];
    let result = divergence_details(&shown, &[]);
    assert!(result.is_empty());
}

#[test]
fn divergence_details_groups_rows_by_artifact() {
    let art = make_doctor_artifact("skew", true);
    let shown = vec![&art];
    let rows = vec![
        make_doctor_row("skew", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skew", "/b/skills", "2.0.0", ArtifactState::Tracked),
    ];
    let result = divergence_details(&shown, &rows);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "skew");
    assert_eq!(result[0].members.len(), 2);
    assert!(!result[0].states_differ);
}

#[test]
fn divergence_details_states_differ_flag() {
    let art = make_doctor_artifact("mixed", true);
    let shown = vec![&art];
    let rows = vec![
        make_doctor_row("mixed", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("mixed", "/b/skills", "1.0.0", ArtifactState::Drifted),
    ];
    let result = divergence_details(&shown, &rows);
    assert_eq!(result.len(), 1);
    assert!(result[0].states_differ, "copies differ in state");
}

#[test]
fn divergence_details_members_sorted_by_location() {
    let art = make_doctor_artifact("sorted", true);
    let shown = vec![&art];
    let rows = vec![
        make_doctor_row("sorted", "/z/skills", "1.0.0", ArtifactState::External),
        make_doctor_row("sorted", "/a/skills", "2.0.0", ArtifactState::External),
    ];
    let result = divergence_details(&shown, &rows);
    assert_eq!(result[0].members[0].location, std::path::PathBuf::from("/a/skills"));
    assert_eq!(result[0].members[1].location, std::path::PathBuf::from("/z/skills"));
}

#[test]
fn empty_system_has_no_issues() {
    let t = TestContext::new();
    let report = survey(false, &t.ctx()).unwrap();
    assert!(report.rows.is_empty());
    assert!(report.missing.is_empty());
    assert!(!report.has_issues());
    assert_eq!(report.counts(), StateCounts::default());
}

#[test]
fn counts_tally_each_state() {
    let t = TestContext::new();
    install_skill(&t, Platform::Claude, "orphan-a", "1.0.0", InstallScope::Global);
    install_skill(&t, Platform::Claude, "orphan-b", "1.0.0", InstallScope::Global);
    let report = survey(false, &t.ctx()).unwrap();
    let c = report.counts();
    assert_eq!(c.orphaned, 2);
    assert_eq!(c.tracked, 0);
    assert_eq!(c.drifted, 0);
}

// --- state_severity ---

#[test]
fn state_severity_exact_values() {
    assert_eq!(state_severity(ArtifactState::Drifted), 4);
    assert_eq!(state_severity(ArtifactState::Orphaned), 3);
    assert_eq!(state_severity(ArtifactState::Untracked), 2);
    assert_eq!(state_severity(ArtifactState::External), 1);
    assert_eq!(state_severity(ArtifactState::Tracked), 0);
}

#[test]
fn state_severity_ordering() {
    assert!(state_severity(ArtifactState::Drifted) > state_severity(ArtifactState::Orphaned));
    assert!(state_severity(ArtifactState::Orphaned) > state_severity(ArtifactState::Untracked));
    assert!(state_severity(ArtifactState::Untracked) > state_severity(ArtifactState::External));
    assert!(state_severity(ArtifactState::External) > state_severity(ArtifactState::Tracked));
}

// --- group_rows ---

#[test]
fn group_rows_same_key_collapses() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "1.0.0", ArtifactState::Tracked),
    ];
    let arts = group_rows(&rows);
    assert_eq!(arts.len(), 1, "two rows with the same (kind, name, scope) collapse to one");
}

#[test]
fn group_rows_different_names_stay_separate() {
    let rows = vec![
        make_doctor_row("alpha", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("beta", "/a/skills", "1.0.0", ArtifactState::Tracked),
    ];
    let arts = group_rows(&rows);
    assert_eq!(arts.len(), 2, "different names produce separate artifacts");
}

#[test]
fn group_rows_diverged_when_states_differ() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "1.0.0", ArtifactState::Drifted),
    ];
    let arts = group_rows(&rows);
    assert!(arts[0].diverged, "different states → diverged");
}

#[test]
fn group_rows_diverged_when_versions_differ() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "2.0.0", ArtifactState::Tracked),
    ];
    let arts = group_rows(&rows);
    assert!(arts[0].diverged, "different versions → diverged");
}

#[test]
fn group_rows_not_diverged_when_copies_agree() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "1.0.0", ArtifactState::Tracked),
    ];
    let arts = group_rows(&rows);
    assert!(!arts[0].diverged, "same state and version → not diverged");
}

#[test]
fn group_rows_state_is_max_severity() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "1.0.0", ArtifactState::Drifted),
    ];
    let arts = group_rows(&rows);
    assert_eq!(arts[0].state, ArtifactState::Drifted, "most-actionable state wins");
}

#[test]
fn group_rows_version_none_when_copies_disagree() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "2.0.0", ArtifactState::Tracked),
    ];
    let arts = group_rows(&rows);
    assert!(arts[0].version.is_none(), "disagreeing versions → no single version");
    let mut vs = arts[0].versions.clone();
    vs.sort();
    assert_eq!(vs, vec!["1.0.0", "2.0.0"], "distinct versions listed");
}

#[test]
fn group_rows_version_some_when_all_agree() {
    let rows = vec![
        make_doctor_row("skill", "/a/skills", "1.0.0", ArtifactState::Tracked),
        make_doctor_row("skill", "/b/skills", "1.0.0", ArtifactState::Tracked),
    ];
    let arts = group_rows(&rows);
    assert_eq!(arts[0].version.as_deref(), Some("1.0.0"), "agreed version is surfaced");
}

#[test]
fn group_rows_tools_is_union_of_tracked_for() {
    let rows = vec![
        DoctorRow {
            kind: ArtifactKind::Skill,
            name: "skill".to_string(),
            scope: InstallScope::Global,
            location: std::path::PathBuf::from("/a"),
            platforms: vec![Platform::Claude],
            tracked_for: vec![Platform::Claude],
            state: ArtifactState::Tracked,
            version: Some("1.0.0".to_string()),
            source: None,
        },
        DoctorRow {
            kind: ArtifactKind::Skill,
            name: "skill".to_string(),
            scope: InstallScope::Global,
            location: std::path::PathBuf::from("/b"),
            platforms: vec![Platform::Pi],
            tracked_for: vec![Platform::Pi],
            state: ArtifactState::Tracked,
            version: Some("1.0.0".to_string()),
            source: None,
        },
    ];
    let arts = group_rows(&rows);
    assert!(arts[0].tools.contains(&Platform::Claude), "claude tracked → in tools");
    assert!(arts[0].tools.contains(&Platform::Pi), "pi tracked → in tools");
}

#[test]
fn group_rows_source_joins_distinct_provenance() {
    let rows = vec![
        DoctorRow {
            kind: ArtifactKind::Skill,
            name: "skill".to_string(),
            scope: InstallScope::Global,
            location: std::path::PathBuf::from("/a"),
            platforms: vec![Platform::Claude],
            tracked_for: vec![],
            state: ArtifactState::Tracked,
            version: Some("1.0.0".to_string()),
            source: Some("repo-a".to_string()),
        },
        DoctorRow {
            kind: ArtifactKind::Skill,
            name: "skill".to_string(),
            scope: InstallScope::Global,
            location: std::path::PathBuf::from("/b"),
            platforms: vec![Platform::Pi],
            tracked_for: vec![],
            state: ArtifactState::Tracked,
            version: Some("1.0.0".to_string()),
            source: Some("repo-b".to_string()),
        },
    ];
    let arts = group_rows(&rows);
    let src = arts[0].source.as_deref().expect("distinct sources joined");
    assert!(src.contains("repo-a") && src.contains("repo-b"), "both repos appear: {src}");
}

#[test]
fn group_rows_source_none_when_no_sources_present() {
    let rows = vec![make_doctor_row(
        "skill",
        "/a",
        "1.0.0",
        ArtifactState::Orphaned,
    )];
    let arts = group_rows(&rows);
    assert!(arts[0].source.is_none(), "orphaned row with no source → None");
}

// --- source_of ---

fn lock_with_entry(name: &str, repo: &str) -> LockFile {
    use crate::types::{LockEntry, LockSource};
    use std::collections::BTreeMap;
    let entry = LockEntry {
        artifact_type: ArtifactKind::Skill,
        version: None,
        installed_at: "2024-01-01T00:00:00Z".to_string(),
        source: LockSource {
            repo: repo.to_string(),
            path: name.to_string(),
        },
        source_checksum: "sha256:abc".to_string(),
        installed_checksum: "sha256:abc".to_string(),
    };
    let mut packages = BTreeMap::new();
    packages.insert(name.to_string(), entry);
    LockFile {
        version: 1,
        packages,
    }
}

fn skill_agg() -> LocationAgg {
    LocationAgg {
        kind: ArtifactKind::Skill,
        scope: InstallScope::Global,
        platforms: vec![Platform::Claude],
    }
}

#[test]
fn source_of_tracked_returns_lock_repo() {
    let agg = skill_agg();
    let mut locks = HashMap::new();
    locks.insert(
        (Platform::Claude, InstallScope::Global),
        lock_with_entry("my-skill", "guidelines"),
    );
    let available: HashMap<(ArtifactKind, String), Vec<String>> = HashMap::new();
    let result = source_of("my-skill", &agg, ArtifactState::Tracked, &locks, &available);
    assert_eq!(result.as_deref(), Some("guidelines"));
}

#[test]
fn source_of_drifted_returns_lock_repo() {
    let agg = skill_agg();
    let mut locks = HashMap::new();
    locks.insert((Platform::Claude, InstallScope::Global), lock_with_entry("my-skill", "my-repo"));
    let available: HashMap<(ArtifactKind, String), Vec<String>> = HashMap::new();
    let result = source_of("my-skill", &agg, ArtifactState::Drifted, &locks, &available);
    assert_eq!(result.as_deref(), Some("my-repo"));
}

#[test]
fn source_of_untracked_returns_providing_source() {
    let agg = skill_agg();
    let locks: HashMap<(Platform, InstallScope), LockFile> = HashMap::new();
    let mut available: HashMap<(ArtifactKind, String), Vec<String>> = HashMap::new();
    available.insert((ArtifactKind::Skill, "loose".to_string()), vec!["community".to_string()]);
    let result = source_of("loose", &agg, ArtifactState::Untracked, &locks, &available);
    assert_eq!(result.as_deref(), Some("community"));
}

#[test]
fn source_of_orphaned_returns_none() {
    let agg = skill_agg();
    let locks: HashMap<(Platform, InstallScope), LockFile> = HashMap::new();
    let available: HashMap<(ArtifactKind, String), Vec<String>> = HashMap::new();
    let result = source_of("hand-authored", &agg, ArtifactState::Orphaned, &locks, &available);
    assert!(result.is_none());
}

#[test]
fn source_of_external_returns_none() {
    let agg = skill_agg();
    let locks: HashMap<(Platform, InstallScope), LockFile> = HashMap::new();
    let available: HashMap<(ArtifactKind, String), Vec<String>> = HashMap::new();
    let result = source_of("ext-tool", &agg, ArtifactState::External, &locks, &available);
    assert!(result.is_none());
}
