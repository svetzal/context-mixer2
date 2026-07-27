//! One decision, one implementation: has this installed copy been hand-edited
//! since it was installed?
//!
//! Before this module existed, that question was answered three separate
//! times: `install.rs`'s `check_local_modifications` resolved the installed
//! path first (propagating `Err` for a kind/platform combination that can't
//! host the artifact) and only then checked existence and looked up the lock
//! entry; `outdated.rs`'s `check_locally_modified` checked the lock entry
//! first (`Ok(false)` when absent) and only then resolved the path; and
//! `info/mod.rs` took a third route keyed only on whether a lock entry
//! existed, with no existence guard at all. Two of the three applied the same
//! guard checks in **opposite order**, so they silently disagreed for an
//! untracked artifact on a platform that doesn't support its kind — one
//! propagated `UnsupportedArtifact`, the other returned `Ok(false)` as if the
//! copy were clean.
//!
//! This module adopts `install.rs`'s order as the one answer: a kind/platform
//! combination that cannot host the artifact is a caller error and must
//! propagate as `Err`, not be swallowed into "not modified."
//!
//! `for_artifact` resolves the installed path (propagating `Err` for an
//! unsupported combination) and delegates to `for_path`, which checks
//! existence, then compares the on-disk checksum against the lock entry.
//! `cmx-core`'s `SkillInstaller` (see `cmx-core/src/skill_install/status.rs`)
//! answers a deliberately narrower, separate question — it already holds a
//! resolved directory path and has no platform-support guard to apply — so it
//! is not routed through this module; see the comment at that call site.

use std::path::Path;

use crate::checksum;
use crate::context::AppContext;
use crate::error::Result;
use crate::types::{ArtifactKind, InstallScope, LockEntry};

/// Whether an installed artifact's on-disk content differs from the checksum
/// recorded in its lock entry at install time.
pub(crate) struct LocalModification {
    /// `true` when the on-disk content no longer matches the lock's recorded
    /// `installed_checksum`.
    pub modified: bool,
    /// The current on-disk checksum, computed only when `modified` is `true`.
    pub disk_checksum: Option<String>,
}

/// Resolve `name`'s installed path for `(kind, scope)` under the active
/// platform, then answer [`for_path`] against it.
///
/// Propagates `Err` when the active platform cannot host artifacts of `kind`
/// (e.g. agents on Pi) — a caller error, not a "clean" installed copy.
pub(crate) fn for_artifact(
    name: &str,
    kind: ArtifactKind,
    scope: InstallScope,
    lock_entry: Option<&LockEntry>,
    ctx: &AppContext<'_>,
) -> Result<LocalModification> {
    let path = ctx.paths.require_installed_artifact_path(kind, name, scope)?;
    for_path(&path, kind, lock_entry, ctx)
}

/// Answer whether the artifact at `path` has been locally modified.
///
/// Returns "not modified" when nothing is installed at `path` (nothing to
/// compare) or when there is no lock entry to compare against (an untracked
/// artifact isn't "modified" — it was never a known baseline). Otherwise
/// compares the current on-disk checksum to the lock entry's recorded
/// `installed_checksum`.
pub(crate) fn for_path(
    path: &Path,
    kind: ArtifactKind,
    lock_entry: Option<&LockEntry>,
    ctx: &AppContext<'_>,
) -> Result<LocalModification> {
    if !ctx.fs.exists(path) {
        return Ok(LocalModification {
            modified: false,
            disk_checksum: None,
        });
    }
    let Some(entry) = lock_entry else {
        return Ok(LocalModification {
            modified: false,
            disk_checksum: None,
        });
    };
    let (modified, disk_checksum) =
        checksum::current_checksum_if_modified(path, kind, entry, ctx.fs)?;
    Ok(LocalModification {
        modified,
        disk_checksum,
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CliError;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, install_agent_on_disk, make_lock_entry_with_checksum};
    use cmx_core::error::CmxError;

    #[test]
    fn no_lock_entry_is_not_modified() {
        let t = TestContext::new();
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", "content", InstallScope::Global);
        let ctx = t.ctx();
        let path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Global)
            .unwrap();

        let result = for_path(&path, ArtifactKind::Agent, None, &ctx).unwrap();
        assert!(!result.modified);
        assert!(result.disk_checksum.is_none());
    }

    #[test]
    fn path_absent_is_not_modified() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "never-installed", InstallScope::Global)
            .unwrap();

        let result = for_path(&path, ArtifactKind::Agent, None, &ctx).unwrap();
        assert!(!result.modified);
        assert!(result.disk_checksum.is_none());
    }

    #[test]
    fn checksum_match_is_not_modified() {
        let t = TestContext::new();
        let content = "---\nname: my-agent\ndescription: test\n---\n";
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", content, InstallScope::Global);
        let ctx = t.ctx();
        let path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Global)
            .unwrap();
        let cs = checksum::checksum_file(&path, &t.fs).unwrap();
        let mut entry =
            make_lock_entry_with_checksum(ArtifactKind::Agent, Some("1.0.0"), "src", "a.md", &cs);
        entry.installed_checksum = cs;

        let result = for_path(&path, ArtifactKind::Agent, Some(&entry), &ctx).unwrap();
        assert!(!result.modified);
        assert!(result.disk_checksum.is_none());
    }

    #[test]
    fn checksum_differs_is_modified() {
        let t = TestContext::new();
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            "current disk content",
            InstallScope::Global,
        );
        let ctx = t.ctx();
        let path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "my-agent", InstallScope::Global)
            .unwrap();
        let mut entry = make_lock_entry_with_checksum(
            ArtifactKind::Agent,
            Some("1.0.0"),
            "src",
            "a.md",
            "sha256:placeholder",
        );
        entry.installed_checksum = "sha256:different_from_disk".to_string();

        let result = for_path(&path, ArtifactKind::Agent, Some(&entry), &ctx).unwrap();
        assert!(result.modified);
        assert!(result.disk_checksum.is_some());
    }

    #[test]
    fn for_artifact_propagates_error_for_unsupported_kind_platform() {
        let t = TestContext::new();
        let pv = t.paths.with_platform(Platform::Pi);
        let pctx = t.ctx().with_paths(&pv);

        let result =
            for_artifact("my-agent", ArtifactKind::Agent, InstallScope::Global, None, &pctx);
        assert!(matches!(result, Err(CliError::Core(CmxError::UnsupportedArtifact { .. }))));
    }
}
