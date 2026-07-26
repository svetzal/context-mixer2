//! One decision, one implementation: which platforms track a given artifact
//! from the canonical home (`<config_dir>/home`, source name [`HOME_SOURCE`])?
//!
//! "Home provenance" is the test a lock entry must pass — `entry.source.repo ==
//! HOME_SOURCE` — to count as tracking the artifact from the canonical home
//! rather than from a registered git source or an external tool. Several
//! commands need this answer: `promote` (which platforms' baselines to refresh
//! after canonicalizing an edit), `sync` (whether `promote` is worth suggesting
//! as an alternative reconciliation), and `adopt`'s `unadopt` (which lock
//! entries to clear when reversing an adoption).
//!
//! Before this module existed, each of those three call sites hand-rolled the
//! same lock-lookup-and-filter loop, and the copies drifted in a way that
//! produced a real, shipped bug: `promote.rs`'s version used `.ok()?` to
//! silently skip a platform whose lock file failed to parse, while `adopt.rs`'s
//! version used `?` to propagate the same error — two different answers to
//! "what happens when a lock file is corrupt" for what is supposed to be one
//! decision. A caller reading only one of the three copies had no way to know
//! the others disagreed. Keeping this decision in one function makes that kind
//! of silent divergence structurally impossible: fix the error-handling policy
//! here once, and every caller gets the same (correct) behavior.
//!
//! `adopt.rs`'s `unadopt_one` had a second, independent bug from the same root
//! cause: because it hand-rolled its own platform loop, it iterated
//! [`crate::platform_iter::all()`] instead of the user's managed-platform
//! allowlist (`cmx config platforms`) — unlike every sibling cross-platform
//! command, which resolves candidates via
//! [`crate::config::managed_or_all_platforms`]. Routing `unadopt` through
//! [`home_tracked_entries`] with the same managed-candidate list every other
//! command uses closes that gap too.

use crate::context::AppContext;
use crate::error::Result;
use crate::lockfile;
use crate::platform::Platform;
use crate::platform_iter;
use crate::types::{ArtifactKind, InstallScope, LockEntry};

/// The canonical source name under which the home is registered.
pub const HOME_SOURCE: &str = "home";

/// Every `(platform, lock entry)` pair for `name` at `scope`, across the
/// `candidates` filtered to platforms that support `kind`. A platform with no
/// lock entry for `name` (or no lock file at all) is simply absent from the
/// result — that is not an error. A lock file that exists but fails to parse
/// *does* propagate as `Err`, so a corrupt lock file never silently drops a
/// platform from the answer.
pub(crate) fn lock_entries_for(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    candidates: &[Platform],
    ctx: &AppContext<'_>,
) -> Result<Vec<(Platform, LockEntry)>> {
    let mut entries = Vec::new();
    for view in platform_iter::views_for(ctx.paths, candidates.iter().copied(), kind) {
        if let Some(entry) = lockfile::load(scope, ctx.fs, &view.paths)?.packages.get(name) {
            entries.push((view.platform, entry.clone()));
        }
    }
    Ok(entries)
}

/// Those entries from [`lock_entries_for`] whose provenance is the canonical
/// home — the answer to "which platforms track this artifact from home?".
pub(crate) fn home_tracked_entries(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    candidates: &[Platform],
    ctx: &AppContext<'_>,
) -> Result<Vec<(Platform, LockEntry)>> {
    Ok(lock_entries_for(name, kind, scope, candidates, ctx)?
        .into_iter()
        .filter(|(_, entry)| entry.source.repo == HOME_SOURCE)
        .collect())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestContext, make_lock_entry_builder, save_lock_with_entry};
    use crate::types::ArtifactKind;

    fn track(t: &TestContext, platform: Platform, name: &str, repo: &str) {
        let entry = make_lock_entry_builder(ArtifactKind::Skill, repo, "skills/x/SKILL.md");
        save_lock_with_entry(
            &t.fs,
            &t.paths.with_platform(platform),
            name,
            entry,
            InstallScope::Global,
        );
    }

    #[test]
    fn home_sourced_entry_is_included() {
        let t = TestContext::new();
        track(&t, Platform::Claude, "my-skill", HOME_SOURCE);

        let entries = home_tracked_entries(
            "my-skill",
            ArtifactKind::Skill,
            InstallScope::Global,
            &[Platform::Claude],
            &t.ctx(),
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, Platform::Claude);
    }

    #[test]
    fn git_sourced_entry_is_excluded() {
        let t = TestContext::new();
        track(&t, Platform::Claude, "my-skill", "some-git-source");

        let entries = home_tracked_entries(
            "my-skill",
            ArtifactKind::Skill,
            InstallScope::Global,
            &[Platform::Claude],
            &t.ctx(),
        )
        .unwrap();

        assert!(entries.is_empty(), "git-sourced entries must not count as home-tracked");
    }

    #[test]
    fn unsupported_platform_is_skipped() {
        let t = TestContext::new();
        // Pi does not support agents.
        let entries = home_tracked_entries(
            "my-agent",
            ArtifactKind::Agent,
            InstallScope::Global,
            &[Platform::Pi],
            &t.ctx(),
        )
        .unwrap();
        assert!(entries.is_empty(), "a platform that doesn't support the kind must be skipped");
    }

    #[test]
    fn missing_lock_file_yields_no_entries_not_an_error() {
        let t = TestContext::new();
        let entries = home_tracked_entries(
            "never-installed",
            ArtifactKind::Skill,
            InstallScope::Global,
            &[Platform::Claude],
            &t.ctx(),
        )
        .unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn lock_entries_for_includes_non_home_entries() {
        let t = TestContext::new();
        track(&t, Platform::Claude, "my-skill", "a-git-source");

        let entries = lock_entries_for(
            "my-skill",
            ArtifactKind::Skill,
            InstallScope::Global,
            &[Platform::Claude],
            &t.ctx(),
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.source.repo, "a-git-source");
    }
}
