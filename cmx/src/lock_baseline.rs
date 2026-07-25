//! Shared primitive for refreshing lock-entry install baselines after
//! reconciling installed content back in line with a canonical copy.
//!
//! Both `cmx skill promote` (canonicalizing an edited installed copy into the
//! home) and `cmx skill sync` (equalizing diverged installed copies to a
//! winner) need to, for a set of platforms that share a lock scope, rewrite
//! each tracked lock entry's `installed_checksum`/`version`/`installed_at` to
//! match freshly-written content. This primitive holds that per-platform
//! mutate loop.
//!
//! Callers differ only in whether `source_checksum` is also updated: promote
//! does (the home *is* the source for a home-provenance artifact, so
//! canonicalizing the installed copy into it means the source now matches
//! too); sync does not (it reconciles installed copies against one another,
//! not against the source, so the source baseline is left untouched).
//!
//! A platform whose lock file has no entry for `name` at `scope` is skipped
//! rather than mutated, so this never creates a lock entry — only refreshes
//! one that already exists.

use crate::context::AppContext;
use crate::error::Result;
use crate::lockfile;
use crate::platform::Platform;
use crate::types::InstallScope;

/// The lock-entry fields a baseline refresh writes. Grouped into a struct
/// (rather than four positional parameters) to keep `refresh_baseline`'s
/// signature readable at call sites.
pub(crate) struct BaselineUpdate<'a> {
    /// New `installed_checksum` value.
    pub checksum: &'a str,
    /// New `version` value (`None` clears it, matching an unversioned copy).
    pub version: Option<&'a str>,
    /// New `source_checksum` value; `None` leaves the existing one untouched.
    pub source_checksum: Option<&'a str>,
    /// New `installed_at` timestamp (RFC 3339).
    pub now: &'a str,
}

/// Refresh `name`'s lock entry at `scope`, for each platform in `platforms`
/// that already tracks it, to the fields in `update`.
pub(crate) fn refresh_baseline(
    name: &str,
    scope: InstallScope,
    platforms: &[Platform],
    update: &BaselineUpdate<'_>,
    ctx: &AppContext<'_>,
) -> Result<()> {
    for &platform in platforms {
        let pv = ctx.paths.with_platform(platform);
        if !lockfile::load(scope, ctx.fs, &pv)?.packages.contains_key(name) {
            continue;
        }
        lockfile::mutate(scope, ctx.fs, &pv, |lock| {
            if let Some(entry) = lock.packages.get_mut(name) {
                entry.installed_checksum = update.checksum.to_string();
                entry.version = update.version.map(str::to_string);
                entry.installed_at = update.now.to_string();
                if let Some(source_checksum) = update.source_checksum {
                    entry.source_checksum = source_checksum.to_string();
                }
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, make_lock_entry_builder, save_lock_with_entry};
    use crate::types::ArtifactKind;

    #[test]
    fn refresh_baseline_updates_checksum_version_and_timestamp() {
        let t = TestContext::new();
        let mut entry = make_lock_entry_builder(ArtifactKind::Skill, "home", "skills/x/SKILL.md");
        entry.installed_checksum = "sha256:old".to_string();
        entry.installed_at = "2020-01-01T00:00:00+00:00".to_string();
        save_lock_with_entry(
            &t.fs,
            &t.paths.with_platform(Platform::Claude),
            "pf",
            entry,
            InstallScope::Global,
        );

        refresh_baseline(
            "pf",
            InstallScope::Global,
            &[Platform::Claude],
            &BaselineUpdate {
                checksum: "sha256:new",
                version: Some("2.0.0"),
                source_checksum: None,
                now: "2026-01-01T00:00:00+00:00",
            },
            &t.ctx(),
        )
        .unwrap();

        let pv = t.paths.with_platform(Platform::Claude);
        let entry = lockfile::load(InstallScope::Global, &t.fs, &pv)
            .unwrap()
            .packages
            .get("pf")
            .cloned()
            .unwrap();
        assert_eq!(entry.installed_checksum, "sha256:new");
        assert_eq!(entry.version.as_deref(), Some("2.0.0"));
        assert_eq!(entry.installed_at, "2026-01-01T00:00:00+00:00");
    }

    #[test]
    fn refresh_baseline_sets_source_checksum_only_when_given() {
        let t = TestContext::new();
        let mut entry = make_lock_entry_builder(ArtifactKind::Skill, "home", "skills/x/SKILL.md");
        entry.source_checksum = "sha256:old-source".to_string();
        save_lock_with_entry(
            &t.fs,
            &t.paths.with_platform(Platform::Claude),
            "pf",
            entry,
            InstallScope::Global,
        );

        refresh_baseline(
            "pf",
            InstallScope::Global,
            &[Platform::Claude],
            &BaselineUpdate {
                checksum: "sha256:new",
                version: None,
                source_checksum: None,
                now: "2026-01-01T00:00:00+00:00",
            },
            &t.ctx(),
        )
        .unwrap();

        let pv = t.paths.with_platform(Platform::Claude);
        let entry = lockfile::load(InstallScope::Global, &t.fs, &pv)
            .unwrap()
            .packages
            .get("pf")
            .cloned()
            .unwrap();
        assert_eq!(entry.source_checksum, "sha256:old-source", "left untouched when None");
    }

    #[test]
    fn refresh_baseline_skips_platforms_without_a_lock_entry() {
        let t = TestContext::new();
        // No lock entry saved for Claude at all.
        refresh_baseline(
            "ghost",
            InstallScope::Global,
            &[Platform::Claude],
            &BaselineUpdate {
                checksum: "sha256:new",
                version: None,
                source_checksum: None,
                now: "2026-01-01T00:00:00+00:00",
            },
            &t.ctx(),
        )
        .unwrap();

        let pv = t.paths.with_platform(Platform::Claude);
        let lock = lockfile::load(InstallScope::Global, &t.fs, &pv).unwrap();
        assert!(!lock.packages.contains_key("ghost"), "no entry created");
    }
}
