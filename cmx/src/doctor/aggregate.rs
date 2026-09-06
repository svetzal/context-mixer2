//! Row aggregation for `cmx doctor`'s survey.
//!
//! Consolidates severity across a logical artifact's copies, folds the raw
//! per-location [`DoctorRow`]s built by `classify.rs` into logical
//! [`DoctorArtifact`]s — deciding representation-aware divergence, where a
//! Codex TOML copy is compared through the Codex projection of its portable
//! siblings rather than by raw bytes — sorts the report's rows and missing
//! entries, and finds lock entries whose artifact file has gone missing from
//! disk.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use crate::context::AppContext;
use crate::platform::Platform;
use crate::types::{InstallScope, LockFile};

use super::types::{ArtifactState, DoctorArtifact, DoctorRow, MissingRow, Representation};

/// Severity ordering used to pick a logical artifact's consolidated state when
/// its copies disagree — the most actionable state wins.
pub(crate) fn state_severity(state: ArtifactState) -> u8 {
    match state {
        ArtifactState::Drifted => 4,
        ArtifactState::Orphaned => 3,
        ArtifactState::Untracked => 2,
        ArtifactState::External => 1,
        ArtifactState::Tracked => 0,
    }
}

/// Group per-location rows into logical artifacts — one per `(kind, name,
/// scope)`, listing every tool it's installed for. A skill installed to several
/// tools collapses to one artifact; it's flagged `diverged` only when its copies'
/// content actually disagrees (see [`content_diverged`]), not merely for
/// existing in more than one place or in more than one representation.
pub(crate) fn group_rows(rows: &[DoctorRow]) -> Vec<DoctorArtifact> {
    // Key by stringified kind so the map key is Ord without needing Ord on ArtifactKind.
    let mut groups: BTreeMap<(String, String, InstallScope), Vec<&DoctorRow>> = BTreeMap::new();
    for row in rows {
        groups
            .entry((row.kind.to_string(), row.name.clone(), row.scope))
            .or_default()
            .push(row);
    }

    groups.into_values().map(|members| fold_group(&members)).collect()
}

/// Whether a logical artifact's copies have genuinely different content.
///
/// Divergence is a content question, independent of version and tracking
/// state — so two copies that happen to share a version (or carry none) but
/// differ in bytes are caught, while byte-identical copies that merely differ
/// in tracking state are not. It is also representation-aware, because an
/// agent's Codex copy is generated TOML and differs from its Markdown sibling
/// *by design*:
///
/// - copies in the same [`Representation`] are diverged when their checksums
///   differ (`portable.len() > 1 || transformed.len() > 1`);
/// - when both representations are present, every Codex TOML checksum must
///   equal the Codex *projection* of some portable copy — what `cmx agent
///   install` would write for Codex from that Markdown today. A projection
///   match means "the same agent, reformatted"; anything else (a hand-edited
///   TOML, one written by an older cmx without the preserved block, or one
///   generated from a different version of the Markdown) is diverged.
///
/// Skills and single-representation groups reduce to the plain checksum rule,
/// and a Codex-only agent can never diverge against a sibling it doesn't have.
fn content_diverged(members: &[&DoctorRow]) -> bool {
    let of = |rep: Representation| members.iter().filter(move |r| r.representation == rep);
    let portable: BTreeSet<&str> =
        of(Representation::Portable).map(|r| r.content_checksum.as_str()).collect();
    let transformed: BTreeSet<&str> =
        of(Representation::CodexToml).map(|r| r.content_checksum.as_str()).collect();
    let projections: BTreeSet<&str> = of(Representation::Portable)
        .filter_map(|r| r.projected_codex_checksum.as_deref())
        .collect();

    portable.len() > 1
        || transformed.len() > 1
        || (!portable.is_empty()
            && !transformed.is_empty()
            && !transformed.iter().all(|cs| projections.contains(cs)))
}

/// Fold one group of per-location rows into a single logical `DoctorArtifact`,
/// consolidating state by severity, detecting content divergence (see
/// [`content_diverged`]), and computing the union of tracked platforms.
fn fold_group(members: &[&DoctorRow]) -> DoctorArtifact {
    let first = members[0];

    // Tools cmx manages this for: the union of each location's
    // tracked-for platforms (lockfile-backed), not every tool that reads
    // a shared directory.
    let mut tools: Vec<Platform> =
        members.iter().flat_map(|r| r.tracked_for.iter().copied()).collect();
    tools.sort_by_key(|p| p.slug());
    tools.dedup();

    let mut locations: Vec<PathBuf> = members.iter().map(|r| r.location.clone()).collect();
    locations.sort();
    locations.dedup();

    let versions: BTreeSet<Option<&str>> = members.iter().map(|r| r.version.as_deref()).collect();
    let source_checksums: BTreeSet<Option<&str>> =
        members.iter().map(|r| r.source_checksum.as_deref()).collect();
    let diverged = content_diverged(members);

    // Consolidated state: the most actionable across copies.
    let state = members
        .iter()
        .map(|r| r.state)
        .max_by_key(|s| state_severity(*s))
        .unwrap_or(first.state);
    // Version only when all copies agree.
    let version = if versions.len() == 1 {
        first.version.clone()
    } else {
        None
    };
    // The distinct versions actually present, sorted — so the display can
    // name a skew (`3.2.0 / 3.3.0`) rather than collapsing to `-`.
    let mut distinct_versions: Vec<String> =
        versions.iter().filter_map(|v| v.map(str::to_string)).collect();
    distinct_versions.sort();
    // Source checksum only when all copies agree, same rule as `version` above.
    let source_checksum = if source_checksums.len() == 1 {
        first.source_checksum.clone()
    } else {
        None
    };

    // Source: the distinct provenance(s) across copies, joined when they
    // differ (rare — copies normally share a source).
    let mut sources: Vec<String> = members.iter().filter_map(|r| r.source.clone()).collect();
    sources.sort();
    sources.dedup();
    let source = if sources.is_empty() {
        None
    } else {
        Some(sources.join(", "))
    };

    DoctorArtifact {
        kind: first.kind,
        name: first.name.clone(),
        scope: first.scope,
        state,
        version,
        versions: distinct_versions,
        tools,
        source,
        source_checksum,
        locations,
        diverged,
    }
}

/// Collect lock entries whose artifact file is gone from disk.
pub(crate) fn collect_missing(
    locks: &HashMap<(Platform, InstallScope), LockFile>,
    ctx: &AppContext<'_>,
) -> Vec<MissingRow> {
    let mut missing = Vec::new();
    for ((platform, scope), lock) in locks {
        let pv = ctx.paths.with_platform(*platform);
        for (name, entry) in &lock.packages {
            let kind = entry.artifact_type;
            if !pv.is_installed(kind, name, *scope, ctx.fs) {
                missing.push(MissingRow {
                    kind,
                    name: name.clone(),
                    scope: *scope,
                    platform: *platform,
                });
            }
        }
    }
    missing
}

pub(crate) fn sort_rows(rows: &mut [DoctorRow]) {
    rows.sort_by(|a, b| {
        a.kind
            .to_string()
            .cmp(&b.kind.to_string())
            .then(a.scope.cmp(&b.scope))
            .then(a.name.cmp(&b.name))
            .then(a.location.cmp(&b.location))
    });
}

pub(crate) fn sort_missing(missing: &mut [MissingRow]) {
    missing.sort_by(|a, b| {
        a.kind
            .to_string()
            .cmp(&b.kind.to_string())
            .then(a.scope.cmp(&b.scope))
            .then(a.name.cmp(&b.name))
            .then(a.platform.slug().cmp(b.platform.slug()))
    });
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::doctor::tests::make_row;
    use crate::platform::Platform;
    use crate::test_support::{
        TestContext, install_skill_on_disk, make_lock_entry_with_checksum, save_lock_with_entry,
        setup_empty_sources,
    };
    use crate::types::{ArtifactKind, InstallScope};

    use super::{collect_missing, group_rows, state_severity};
    use crate::doctor::types::{ArtifactState, DoctorArtifact, DoctorRow, Representation};

    /// An agent copy at `location` held in `representation`, with the given
    /// content checksum and (for portable copies) Codex projection checksum.
    fn agent_row(
        location: &str,
        representation: Representation,
        checksum: &str,
        projection: Option<&str>,
    ) -> DoctorRow {
        let mut row = make_row(ArtifactKind::Agent, "reviewer", ArtifactState::Tracked, checksum);
        row.location = PathBuf::from(location);
        row.platforms = match representation {
            Representation::Portable => vec![Platform::Claude],
            Representation::CodexToml => vec![Platform::Codex],
        };
        row.tracked_for = row.platforms.clone();
        row.representation = representation;
        row.projected_codex_checksum = projection.map(str::to_string);
        row
    }

    fn markdown_row(location: &str, checksum: &str, projection: &str) -> DoctorRow {
        agent_row(location, Representation::Portable, checksum, Some(projection))
    }

    fn codex_row(checksum: &str) -> DoctorRow {
        agent_row("/home/testuser/.codex/agents", Representation::CodexToml, checksum, None)
    }

    // -----------------------------------------------------------------------
    // state_severity — ordering must be Drifted > Orphaned > Untracked > External > Tracked
    // -----------------------------------------------------------------------

    #[test]
    fn state_severity_ordering() {
        let drifted = state_severity(ArtifactState::Drifted);
        let orphaned = state_severity(ArtifactState::Orphaned);
        let untracked = state_severity(ArtifactState::Untracked);
        let external = state_severity(ArtifactState::External);
        let tracked = state_severity(ArtifactState::Tracked);

        assert!(drifted > orphaned, "Drifted must outrank Orphaned");
        assert!(orphaned > untracked, "Orphaned must outrank Untracked");
        assert!(untracked > external, "Untracked must outrank External");
        assert!(external > tracked, "External must outrank Tracked");
    }

    // -----------------------------------------------------------------------
    // group_rows / fold_group
    // -----------------------------------------------------------------------

    #[test]
    fn group_rows_collapses_same_kind_name_scope() {
        let rows = vec![
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x"),
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x"),
        ];
        let artifacts: Vec<DoctorArtifact> = group_rows(&rows);
        assert_eq!(artifacts.len(), 1, "two rows with the same (kind, name, scope) must collapse");
    }

    #[test]
    fn group_rows_keeps_different_names_separate() {
        let rows = vec![
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x"),
            make_row(ArtifactKind::Skill, "beta", ArtifactState::Tracked, "sha256:y"),
        ];
        let artifacts = group_rows(&rows);
        assert_eq!(artifacts.len(), 2);
    }

    #[test]
    fn group_rows_diverged_only_when_checksums_differ() {
        // Same version, different state — but same content bytes → NOT diverged
        let mut row_a =
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:same");
        row_a.location = PathBuf::from("/path/a");
        row_a.tracked_for = vec![Platform::Claude];

        let mut row_b =
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Untracked, "sha256:same");
        row_b.location = PathBuf::from("/path/b");
        row_b.tracked_for = vec![];

        let artifacts = group_rows(&[row_a, row_b]);
        assert_eq!(artifacts.len(), 1);
        assert!(!artifacts[0].diverged, "byte-identical copies must not be flagged diverged");
    }

    #[test]
    fn group_rows_diverged_when_checksums_differ() {
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v1");
        row_a.location = PathBuf::from("/path/a");

        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v2");
        row_b.location = PathBuf::from("/path/b");

        let artifacts = group_rows(&[row_a, row_b]);
        assert!(
            artifacts[0].diverged,
            "copies with different checksums must be flagged diverged"
        );
    }

    #[test]
    fn skill_copies_with_different_checksums_still_diverge() {
        // Skills are always portable and never projected; the plain checksum
        // rule must survive the representation-aware refactor unchanged.
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v1");
        row_a.location = PathBuf::from("/path/a");
        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v2");
        row_b.location = PathBuf::from("/path/b");
        for row in [&row_a, &row_b] {
            assert_eq!(row.representation, Representation::Portable);
            assert_eq!(row.projected_codex_checksum, None);
        }

        let artifacts = group_rows(&[row_a, row_b]);
        assert!(artifacts[0].diverged);
    }

    // -----------------------------------------------------------------------
    // content_diverged — representation-aware rule
    // -----------------------------------------------------------------------

    #[test]
    fn codex_copy_matching_the_markdown_projection_is_not_diverged() {
        let rows = vec![
            markdown_row("/home/testuser/.claude/agents", "sha256:md", "sha256:proj"),
            codex_row("sha256:proj"),
        ];
        let artifacts = group_rows(&rows);
        assert!(
            !artifacts[0].diverged,
            "a Codex TOML that is exactly the projection of its Markdown sibling is the same agent reformatted"
        );
    }

    #[test]
    fn codex_copy_not_matching_the_markdown_projection_is_diverged() {
        let rows = vec![
            markdown_row("/home/testuser/.claude/agents", "sha256:md", "sha256:proj"),
            codex_row("sha256:edited"),
        ];
        let artifacts = group_rows(&rows);
        assert!(artifacts[0].diverged, "an edited/stale/older-format TOML is diverged");
    }

    #[test]
    fn codex_copy_matching_any_portable_projection_is_not_diverged() {
        // Two byte-identical Markdown copies both project to the same TOML;
        // the Codex copy matches that projection.
        let rows = vec![
            markdown_row("/home/testuser/.claude/agents", "sha256:md", "sha256:proj"),
            markdown_row("/home/testuser/.config/opencode/agent", "sha256:md", "sha256:proj"),
            codex_row("sha256:proj"),
        ];
        assert!(!group_rows(&rows)[0].diverged);
    }

    #[test]
    fn codex_only_agent_is_never_diverged() {
        let artifacts = group_rows(&[codex_row("sha256:toml")]);
        assert!(!artifacts[0].diverged, "nothing to compare against");
    }

    #[test]
    fn two_codex_copies_with_different_bytes_are_diverged() {
        let mut local = codex_row("sha256:toml-b");
        local.location = PathBuf::from("/project/.codex/agents");
        let rows = vec![codex_row("sha256:toml-a"), local];
        assert!(group_rows(&rows)[0].diverged);
    }

    #[test]
    fn portable_agent_copies_are_compared_by_bytes() {
        let same = vec![
            markdown_row("/home/testuser/.claude/agents", "sha256:md", "sha256:proj"),
            markdown_row("/home/testuser/.config/opencode/agent", "sha256:md", "sha256:proj"),
        ];
        assert!(!group_rows(&same)[0].diverged, "identical Markdown copies");

        let different = vec![
            markdown_row("/home/testuser/.claude/agents", "sha256:md-1", "sha256:proj-1"),
            markdown_row("/home/testuser/.config/opencode/agent", "sha256:md-2", "sha256:proj-2"),
        ];
        assert!(group_rows(&different)[0].diverged, "different Markdown copies");
    }

    #[test]
    fn portable_copy_without_a_projection_cannot_vouch_for_a_codex_copy() {
        // The Markdown could not be read back (no projection), so the Codex
        // copy has nothing to match: report divergence rather than guess.
        let rows = vec![
            agent_row("/home/testuser/.claude/agents", Representation::Portable, "sha256:md", None),
            codex_row("sha256:proj"),
        ];
        assert!(group_rows(&rows)[0].diverged);
    }

    #[test]
    fn group_rows_consolidated_state_is_highest_severity() {
        let mut row_a =
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:same");
        row_a.location = PathBuf::from("/path/a");

        let mut row_b =
            make_row(ArtifactKind::Skill, "alpha", ArtifactState::Drifted, "sha256:same");
        row_b.location = PathBuf::from("/path/b");

        let artifacts = group_rows(&[row_a, row_b]);
        assert_eq!(
            artifacts[0].state,
            ArtifactState::Drifted,
            "consolidated state must be the highest-severity copy"
        );
    }

    #[test]
    fn group_rows_tools_is_union_of_tracked_for_not_platforms() {
        // row_a is tracked for Claude; row_b has Claude in platforms but NOT in tracked_for
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row_a.tracked_for = vec![Platform::Claude];
        row_a.platforms = vec![Platform::Claude];
        row_a.location = PathBuf::from("/path/a");

        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Orphaned, "sha256:x");
        row_b.tracked_for = vec![]; // NOT tracked for anyone
        row_b.platforms = vec![Platform::Claude]; // but Claude reads this dir
        row_b.location = PathBuf::from("/path/b");

        let artifacts = group_rows(&[row_a, row_b]);
        // tools must come from tracked_for only
        assert_eq!(artifacts[0].tools, vec![Platform::Claude]);
        // The orphaned copy has Claude in platforms but not tracked_for;
        // that must not inflate the tools list
        assert_eq!(artifacts[0].tools.len(), 1);
    }

    #[test]
    fn group_rows_version_none_when_copies_disagree() {
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v1");
        row_a.location = PathBuf::from("/path/a");
        row_a.version = Some("3.2.0".to_string());

        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v2");
        row_b.location = PathBuf::from("/path/b");
        row_b.version = Some("3.3.0".to_string());

        let artifacts = group_rows(&[row_a, row_b]);
        assert!(artifacts[0].version.is_none(), "version must be None when copies disagree");
        // But distinct_versions must carry both
        let mut versions = artifacts[0].versions.clone();
        versions.sort();
        assert_eq!(versions, vec!["3.2.0", "3.3.0"]);
    }

    #[test]
    fn group_rows_source_checksum_carried_for_single_location() {
        let mut row = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row.source_checksum = Some("sha256:source-x".to_string());

        let artifacts = group_rows(&[row]);
        assert_eq!(artifacts[0].source_checksum.as_deref(), Some("sha256:source-x"));
    }

    #[test]
    fn group_rows_source_checksum_none_when_copies_disagree() {
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v1");
        row_a.location = PathBuf::from("/path/a");
        row_a.source_checksum = Some("sha256:source-a".to_string());

        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:v2");
        row_b.location = PathBuf::from("/path/b");
        row_b.source_checksum = Some("sha256:source-b".to_string());

        let artifacts = group_rows(&[row_a, row_b]);
        assert!(
            artifacts[0].source_checksum.is_none(),
            "source_checksum must be None when copies disagree"
        );
    }

    #[test]
    fn group_rows_source_joined_when_distinct() {
        let mut row_a = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row_a.location = PathBuf::from("/path/a");
        row_a.source = Some("source-a".to_string());

        let mut row_b = make_row(ArtifactKind::Skill, "alpha", ArtifactState::Tracked, "sha256:x");
        row_b.location = PathBuf::from("/path/b");
        row_b.source = Some("source-b".to_string());

        let artifacts = group_rows(&[row_a, row_b]);
        let src = artifacts[0].source.as_deref().unwrap_or("");
        assert!(src.contains("source-a"), "joined source must include source-a");
        assert!(src.contains("source-b"), "joined source must include source-b");
    }

    // -----------------------------------------------------------------------
    // collect_missing
    // -----------------------------------------------------------------------

    #[test]
    fn collect_missing_returns_absent_artifact() {
        let t = TestContext::new();
        // Seed a lock entry for "missing-skill" but do NOT put a file on disk
        let entry = make_lock_entry_with_checksum(
            ArtifactKind::Skill,
            None,
            "test-source",
            "skills/missing-skill",
            "sha256:abc",
        );
        save_lock_with_entry(&t.fs, &t.paths, "missing-skill", entry, InstallScope::Global);
        setup_empty_sources(&t.fs, &t.paths);

        let mut locks = HashMap::new();
        let lock = crate::lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        locks.insert((Platform::Claude, InstallScope::Global), lock);

        let ctx = t.ctx();
        let missing = collect_missing(&locks, &ctx);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "missing-skill");
        assert_eq!(missing[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn collect_missing_does_not_report_present_artifact() {
        let t = TestContext::new();
        let entry = make_lock_entry_with_checksum(
            ArtifactKind::Skill,
            None,
            "test-source",
            "skills/present-skill",
            "sha256:abc",
        );
        save_lock_with_entry(&t.fs, &t.paths, "present-skill", entry, InstallScope::Global);
        // Actually install the skill on disk
        install_skill_on_disk(
            &t.fs,
            &t.paths,
            "present-skill",
            "---\n---\n# skill\n",
            InstallScope::Global,
        );
        setup_empty_sources(&t.fs, &t.paths);

        let mut locks = HashMap::new();
        let lock = crate::lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        locks.insert((Platform::Claude, InstallScope::Global), lock);

        let ctx = t.ctx();
        let missing = collect_missing(&locks, &ctx);
        assert!(missing.is_empty(), "present artifact must not appear in missing");
    }
}
