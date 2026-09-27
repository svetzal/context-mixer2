//! Suggestion helpers for commands.

use std::collections::{BTreeMap, BTreeSet};

use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::platform::Platform;
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope};

/// Which install scope(s) the command reporting "not installed" looked in —
/// so a hint knows whether re-running needs a different scope as well as (or
/// instead of) a different platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchedScope {
    /// The command looks in both scopes on its own (e.g. `update`, `info`,
    /// `diff`, `promote`, `set add`); scope never needs changing on a re-run.
    Both,
    /// The command looked in just this scope, chosen by its `--local` flag
    /// (e.g. `uninstall`, `sync`); a copy at the other scope needs the flag
    /// added or dropped.
    Only(InstallScope),
}

/// Where each installed name is tracked: name → every `(platform, scope)` whose
/// lock file records it, in managed-platform then scope order.
type TrackedLocations = BTreeMap<String, Vec<(Platform, InstallScope)>>;

/// Build a hint for a name that wasn't found among installed artifacts.
///
/// When the exact name *is* tracked — just not where the command looked — the
/// hint says where and gives a re-run that finds it: `--platform <first>` for
/// another platform, plus `--local` (or dropping it) when the copy is at a
/// scope the command did not search (`searched`). Otherwise it offers a "did you mean" for a
/// near miss (never the identical name), falling back to a generic pointer to
/// `cmx list`.
pub fn installed_artifact_hint(
    name: &str,
    kind: Option<ArtifactKind>,
    searched: SearchedScope,
    ctx: &AppContext<'_>,
) -> String {
    let tracked = installed_candidates(kind, ctx).unwrap_or_default();
    if let Some(locations) = tracked.get(name) {
        return tracked_elsewhere_hint(locations, ctx.paths.platform, searched);
    }
    let names: BTreeSet<String> = tracked.into_keys().collect();
    hint_from_candidates(name, &names).unwrap_or_else(|| match kind {
        Some(kind) => format!("See 'cmx {kind} list'."),
        None => "See 'cmx list'.".to_string(),
    })
}

/// Build a "did you mean" hint for a name that wasn't found among source
/// artifacts, falling back to a generic pointer to `cmx search` when no close
/// match exists.
pub fn source_artifact_hint(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> String {
    let candidates = source_candidates(kind, ctx).unwrap_or_default();
    hint_from_candidates(name, &candidates).unwrap_or_else(|| format!("See 'cmx search {name}'."))
}

/// Say where an exactly-named artifact is tracked — and how to re-run so the
/// next attempt finds it — given every location tracking it and the scope(s)
/// the failing command `searched`.
///
/// 1. Another platform tracks it at a scope the command already looks in: only
///    the platform needs changing, so the hint stays short
///    (`Re-run with '--platform codex'.`).
/// 2. It is tracked only at the scope the command did not look in: the hint
///    names that scope and adds `--local` (or says to drop it), plus
///    `--platform` unless the active platform is among those tracking it.
/// 3. Otherwise only the active platform tracks it, somewhere the command did
///    look; the hint just names the scope(s).
fn tracked_elsewhere_hint(
    locations: &[(Platform, InstallScope)],
    active: Platform,
    searched: SearchedScope,
) -> String {
    let searched_here = |scope: InstallScope| match searched {
        SearchedScope::Both => true,
        SearchedScope::Only(searched_scope) => scope == searched_scope,
    };
    let others = distinct_platforms(locations, |p, scope| p != active && searched_here(scope));
    if let Some(first) = others.first() {
        return format!("It is installed for {}. Re-run with '--platform {first}'.", list(&others));
    }

    if let SearchedScope::Only(searched_scope) = searched {
        let other_scope = match searched_scope {
            InstallScope::Global => InstallScope::Local,
            InstallScope::Local => InstallScope::Global,
        };
        let there = distinct_platforms(locations, |_, scope| scope == other_scope);
        if let Some(first) = there.first() {
            let rerun = match (there.contains(&active), other_scope) {
                (true, InstallScope::Local) => "with '--local'".to_string(),
                (true, InstallScope::Global) => "without '--local'".to_string(),
                (false, InstallScope::Local) => format!("with '--platform {first} --local'"),
                (false, InstallScope::Global) => {
                    format!("with '--platform {first}' and without '--local'")
                }
            };
            return format!(
                "It is installed for {} at {} scope. Re-run {rerun}.",
                list(&there),
                other_scope.label()
            );
        }
    }

    let mut scopes: Vec<&str> = Vec::new();
    for &(_, scope) in locations {
        if !scopes.contains(&scope.label()) {
            scopes.push(scope.label());
        }
    }
    format!("It is installed for {active} at {} scope.", scopes.join(" and "))
}

/// The distinct platforms among `locations` that satisfy `keep`, in order.
fn distinct_platforms(
    locations: &[(Platform, InstallScope)],
    keep: impl Fn(Platform, InstallScope) -> bool,
) -> Vec<Platform> {
    let mut platforms: Vec<Platform> = Vec::new();
    for &(platform, scope) in locations {
        if keep(platform, scope) && !platforms.contains(&platform) {
            platforms.push(platform);
        }
    }
    platforms
}

fn list(platforms: &[Platform]) -> String {
    platforms.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

fn installed_candidates(
    kind: Option<ArtifactKind>,
    ctx: &AppContext<'_>,
) -> crate::error::Result<TrackedLocations> {
    let mut tracked = TrackedLocations::new();
    for platform in config::managed_or_all_platforms(ctx.fs, ctx.paths)? {
        let paths = ctx.paths.with_platform(platform);
        for scope in InstallScope::ALL {
            let lock = lockfile::load(scope, ctx.fs, &paths)?;
            for (name, entry) in lock.packages {
                if kind.is_none_or(|expected| entry.artifact_type == expected) {
                    tracked.entry(name).or_default().push((platform, scope));
                }
            }
        }
    }
    Ok(tracked)
}

fn source_candidates(
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> crate::error::Result<BTreeSet<String>> {
    Ok(source_iter::all_artifacts(ctx)?
        .into_iter()
        .filter(|artifact| artifact.artifact.kind == kind)
        .map(|artifact| artifact.artifact.name)
        .collect())
}

fn hint_from_candidates(name: &str, candidates: &BTreeSet<String>) -> Option<String> {
    let best = candidates
        .iter()
        .map(|candidate| (candidate, levenshtein(name, candidate)))
        .filter(|(candidate, distance)| *distance <= max_distance(name, candidate))
        .min_by(|(left_name, left_distance), (right_name, right_distance)| {
            left_distance.cmp(right_distance).then_with(|| left_name.cmp(right_name))
        })?;

    Some(format!("Did you mean '{}'?", best.0))
}

fn max_distance(left: &str, right: &str) -> usize {
    match left.chars().count().max(right.chars().count()) {
        0..=4 => 1,
        5..=8 => 2,
        _ => 3,
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0; right_chars.len() + 1];

    for (left_index, left_char) in left.chars().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != *right_char);
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + cost);
        }
        previous.clone_from(&current);
    }

    previous[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::{SearchedScope, installed_artifact_hint, levenshtein, tracked_elsewhere_hint};
    use crate::config;
    use crate::lockfile;
    use crate::platform::Platform;
    use crate::test_support::{TestContext, sample_lock_entry};
    use crate::types::{ArtifactKind, CmxConfig, InstallScope, LockFile};
    use std::collections::BTreeMap;

    #[test]
    fn levenshtein_handles_near_miss() {
        assert_eq!(levenshtein("focus-skll", "focus-skill"), 1);
    }

    #[test]
    fn installed_hint_prefers_close_match() {
        let t = TestContext::new();
        let mut packages = BTreeMap::new();
        let mut entry = sample_lock_entry();
        entry.artifact_type = ArtifactKind::Skill;
        packages.insert("focus-skill".to_string(), entry);
        lockfile::save(
            &LockFile {
                version: 1,
                packages,
            },
            InstallScope::Global,
            &t.fs,
            &t.paths,
        )
        .unwrap();

        let hint = installed_artifact_hint(
            "focus-skll",
            Some(ArtifactKind::Skill),
            SearchedScope::Both,
            &t.ctx(),
        );
        assert_eq!(hint, "Did you mean 'focus-skill'?");
    }

    #[test]
    fn installed_hint_ignores_unmanaged_platforms() {
        let t = TestContext::new();
        let cursor_paths = t.paths.with_platform(Platform::Cursor);
        let mut packages = BTreeMap::new();
        let mut entry = sample_lock_entry();
        entry.artifact_type = ArtifactKind::Skill;
        packages.insert("focus-skill".to_string(), entry);
        lockfile::save(
            &LockFile {
                version: 1,
                packages,
            },
            InstallScope::Global,
            &t.fs,
            &cursor_paths,
        )
        .unwrap();

        let cfg = CmxConfig {
            platforms: vec![Platform::Claude],
            ..Default::default()
        };
        config::save_config(&cfg, &t.fs, &t.paths).unwrap();

        let hint = installed_artifact_hint(
            "focus-skll",
            Some(ArtifactKind::Skill),
            SearchedScope::Both,
            &t.ctx(),
        );
        assert_eq!(hint, "See 'cmx skill list'.");
    }

    #[test]
    fn tracked_elsewhere_hint_names_both_scopes_on_the_active_platform() {
        let hint = tracked_elsewhere_hint(
            &[
                (Platform::Claude, InstallScope::Global),
                (Platform::Claude, InstallScope::Local),
            ],
            Platform::Claude,
            SearchedScope::Both,
        );
        assert_eq!(hint, "It is installed for claude at global and local scope.");
    }

    fn track(
        t: &TestContext,
        platform: Platform,
        scope: InstallScope,
        name: &str,
        kind: ArtifactKind,
    ) {
        let pv = t.paths.with_platform(platform);
        let mut lock = lockfile::load(scope, &t.fs, &pv).unwrap();
        let mut entry = sample_lock_entry();
        entry.artifact_type = kind;
        lock.packages.insert(name.to_string(), entry);
        lockfile::save(&lock, scope, &t.fs, &pv).unwrap();
    }

    fn hint_on(
        t: &TestContext,
        active: Platform,
        name: &str,
        kind: Option<ArtifactKind>,
    ) -> String {
        let pv = t.paths.with_platform(active);
        installed_artifact_hint(name, kind, SearchedScope::Both, &t.ctx().with_paths(&pv))
    }

    #[test]
    fn installed_hint_names_the_platform_tracking_the_exact_name() {
        let t = TestContext::new();
        track(&t, Platform::Claude, InstallScope::Global, "uv-python", ArtifactKind::Agent);

        let hint = hint_on(&t, Platform::Codex, "uv-python", Some(ArtifactKind::Agent));
        assert_eq!(hint, "It is installed for claude. Re-run with '--platform claude'.");
    }

    #[test]
    fn installed_hint_lists_every_platform_tracking_the_exact_name() {
        let t = TestContext::new();
        track(&t, Platform::Claude, InstallScope::Global, "focus", ArtifactKind::Skill);
        track(&t, Platform::Cursor, InstallScope::Local, "focus", ArtifactKind::Skill);
        track(&t, Platform::Claude, InstallScope::Local, "focus", ArtifactKind::Skill);

        let hint = hint_on(&t, Platform::Codex, "focus", Some(ArtifactKind::Skill));
        assert_eq!(hint, "It is installed for claude, cursor. Re-run with '--platform claude'.");
    }

    #[test]
    fn installed_hint_ignores_an_exact_name_of_another_kind() {
        let t = TestContext::new();
        track(&t, Platform::Claude, InstallScope::Global, "focus", ArtifactKind::Agent);

        let hint = hint_on(&t, Platform::Codex, "focus", Some(ArtifactKind::Skill));
        assert_eq!(hint, "See 'cmx skill list'.");
    }

    #[test]
    fn installed_hint_names_the_scope_when_only_the_active_platform_tracks_it() {
        let t = TestContext::new();
        track(&t, Platform::Claude, InstallScope::Local, "focus", ArtifactKind::Skill);

        let hint = hint_on(&t, Platform::Claude, "focus", Some(ArtifactKind::Skill));
        assert_eq!(hint, "It is installed for claude at local scope.");
    }

    #[test]
    fn installed_hint_never_suggests_the_identical_name() {
        let t = TestContext::new();
        track(&t, Platform::Claude, InstallScope::Global, "focus-skill", ArtifactKind::Skill);
        track(&t, Platform::Claude, InstallScope::Global, "focus-skil", ArtifactKind::Skill);

        let hint = hint_on(&t, Platform::Codex, "focus-skill", Some(ArtifactKind::Skill));
        assert!(!hint.contains("Did you mean"), "{hint}");
    }

    // --- scope-aware re-run suggestions ---

    const G: InstallScope = InstallScope::Global;
    const L: InstallScope = InstallScope::Local;

    #[test]
    fn a_command_searching_both_scopes_never_mentions_scope() {
        let hint =
            tracked_elsewhere_hint(&[(Platform::Codex, L)], Platform::Claude, SearchedScope::Both);
        assert_eq!(hint, "It is installed for codex. Re-run with '--platform codex'.");
    }

    #[test]
    fn same_scope_on_another_platform_needs_only_the_platform() {
        let hint = tracked_elsewhere_hint(
            &[(Platform::Codex, G), (Platform::Cursor, L)],
            Platform::Claude,
            SearchedScope::Only(G),
        );
        assert_eq!(hint, "It is installed for codex. Re-run with '--platform codex'.");
    }

    #[test]
    fn local_scope_on_another_platform_needs_platform_and_local() {
        let hint = tracked_elsewhere_hint(
            &[(Platform::Codex, L)],
            Platform::Claude,
            SearchedScope::Only(G),
        );
        assert_eq!(
            hint,
            "It is installed for codex at local scope. Re-run with '--platform codex --local'."
        );
    }

    #[test]
    fn global_scope_on_another_platform_needs_platform_and_no_local() {
        let hint = tracked_elsewhere_hint(
            &[(Platform::Codex, G)],
            Platform::Claude,
            SearchedScope::Only(L),
        );
        assert_eq!(
            hint,
            "It is installed for codex at global scope. \
             Re-run with '--platform codex' and without '--local'."
        );
    }

    #[test]
    fn other_scope_on_the_active_platform_needs_only_the_scope_flag() {
        let hint = tracked_elsewhere_hint(
            &[(Platform::Claude, L), (Platform::Codex, L)],
            Platform::Claude,
            SearchedScope::Only(G),
        );
        assert_eq!(
            hint,
            "It is installed for claude, codex at local scope. Re-run with '--local'."
        );
        let hint = tracked_elsewhere_hint(
            &[(Platform::Claude, G)],
            Platform::Claude,
            SearchedScope::Only(L),
        );
        assert_eq!(hint, "It is installed for claude at global scope. Re-run without '--local'.");
    }
}
