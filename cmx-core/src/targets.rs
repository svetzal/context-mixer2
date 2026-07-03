//! Target platform resolution for install operations.
//!
//! This module provides the shared [`resolve_targets`] function used by both
//! the `cmx` CLI (`cmx skill install`) and the embeddable `SkillInstaller` API.
//! The logic is the same in both cases:
//!
//! - An explicit selector (`Some(p)`) → just that platform.
//! - Explicit managed set in config → every managed platform that supports `kind`.
//! - No managed set → every platform with a non-empty lock file at `scope`
//!   (i.e. platforms "in use"), falling back to `[Claude]` on a fresh machine.

use anyhow::Result;

use crate::context::AppContext;
use crate::lockfile;
use crate::platform::Platform;
use crate::platform_iter;
use crate::types::{ArtifactKind, InstallScope};

/// Resolve the platforms a default (no `--platform`) install should target.
///
/// See module-level documentation for the resolution rules.
pub fn resolve_targets(
    selector: Option<Platform>,
    kind: ArtifactKind,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<Vec<Platform>> {
    if let Some(p) = selector {
        return Ok(vec![p]);
    }
    if let Some(managed) = crate::config::managed_platforms(ctx.fs, ctx.paths)? {
        return Ok(managed.into_iter().filter(|p| p.supports(kind)).collect());
    }
    let mut targets = Vec::new();
    for view in platform_iter::views_for(ctx.paths, platform_iter::all(), kind) {
        if !lockfile::load(scope, ctx.fs, &view.paths)?.packages.is_empty() {
            targets.push(view.platform);
        }
    }
    if targets.is_empty() {
        targets.push(Platform::Claude);
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::test_support::{TestContext, sample_lock_entry, save_lock_with_entry};
    use crate::types::{ArtifactKind, CmxConfig, InstallScope};

    #[test]
    fn fresh_machine_returns_claude() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let targets =
            resolve_targets(None, ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();
        assert_eq!(targets, vec![Platform::Claude]);
    }

    #[test]
    fn explicit_selector_returns_that_platform() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let targets =
            resolve_targets(Some(Platform::Codex), ArtifactKind::Skill, InstallScope::Global, &ctx)
                .unwrap();
        assert_eq!(targets, vec![Platform::Codex]);
    }

    #[test]
    fn managed_platforms_override_inference() {
        let t = TestContext::new();
        let cfg = CmxConfig {
            platforms: vec![Platform::Claude, Platform::Codex],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let ctx = t.ctx();
        let targets =
            resolve_targets(None, ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();
        assert_eq!(targets, vec![Platform::Claude, Platform::Codex]);
    }

    #[test]
    fn non_empty_codex_lock_adds_codex_to_targets() {
        let t = TestContext::new();
        let codex_paths = t.paths.with_platform(Platform::Codex);
        save_lock_with_entry(
            &t.fs,
            &codex_paths,
            "some-skill",
            sample_lock_entry(),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let targets =
            resolve_targets(None, ArtifactKind::Skill, InstallScope::Global, &ctx).unwrap();
        assert!(targets.contains(&Platform::Codex), "Codex has a non-empty lock");
    }
}
