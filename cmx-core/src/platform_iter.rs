//! Shared iterators for cross-platform traversal.
//!
//! The `for platform in Platform::ALL { if !platform.supports(kind) { continue }
//! let pv = ctx.paths.with_platform(platform); ... }` pattern appears throughout
//! the codebase. This module centralises it so every call site reads as a
//! one-liner filter-map rather than a three-line hand-rolled guard.
//!
//! # Usage
//!
//! ```no_run
//! # use cmx_core::platform_iter;
//! # use cmx_core::types::ArtifactKind;
//! # use cmx_core::context::AppContext;
//! // Iterate every platform that supports `kind`:
//! // for view in platform_iter::views_for(ctx, platform_iter::all(), kind) { ... }
//! //
//! // Put the active platform first:
//! // for view in platform_iter::views_for(ctx, platform_iter::active_first(active), kind) { ... }
//! ```

use std::iter;

use crate::paths::ConfigPaths;
use crate::platform::Platform;
use crate::types::ArtifactKind;

/// A platform together with the `ConfigPaths` view scoped to it.
///
/// Callers use `view.paths` wherever a platform-specific `ConfigPaths` is
/// needed, and `view.platform` for display or comparisons.
pub struct PlatformView {
    pub platform: Platform,
    pub paths: ConfigPaths,
}

/// Filter `platforms` to those that support `kind` and map each to a
/// [`PlatformView`] that bundles the platform with its scoped [`ConfigPaths`].
///
/// The base paths (`home_dir`, `config_dir`) are cloned from `base_paths` into
/// each view via [`ConfigPaths::with_platform`]; only the active `platform`
/// field changes.
pub fn views_for(
    base_paths: &ConfigPaths,
    platforms: impl IntoIterator<Item = Platform>,
    kind: ArtifactKind,
) -> impl Iterator<Item = PlatformView> {
    // Pre-clone both path roots so the closure owns them and does not hold a
    // reference to `base_paths` across the iterator's lifetime.
    let config_dir = base_paths.config_dir.clone();
    let home_dir = base_paths.home_dir.clone();
    platforms
        .into_iter()
        .filter(move |p| p.supports(kind))
        .map(move |platform| PlatformView {
            paths: ConfigPaths {
                config_dir: config_dir.clone(),
                home_dir: home_dir.clone(),
                platform,
            },
            platform,
        })
}

/// Iterate every platform in [`Platform::ALL`] in canonical order.
pub fn all() -> impl Iterator<Item = Platform> {
    Platform::ALL.iter().copied()
}

/// Iterate `active` first, then every other platform in canonical order.
///
/// Mirrors the pattern in `info/mod.rs`: the active platform is searched first
/// so a locally-installed artifact is found without scanning every tool.
pub fn active_first(active: Platform) -> impl Iterator<Item = Platform> {
    iter::once(active).chain(Platform::ALL.iter().copied().filter(move |&p| p != active))
}
