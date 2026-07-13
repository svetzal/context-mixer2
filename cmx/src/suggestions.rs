use std::collections::BTreeSet;

use crate::context::AppContext;
use crate::lockfile;
use crate::platform::Platform;
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope};

pub fn installed_artifact_hint(
    name: &str,
    kind: Option<ArtifactKind>,
    ctx: &AppContext<'_>,
) -> String {
    let candidates = installed_candidates(kind, ctx).unwrap_or_default();
    hint_from_candidates(name, &candidates).unwrap_or_else(|| match kind {
        Some(kind) => format!("See 'cmx {kind} list'."),
        None => "See 'cmx list'.".to_string(),
    })
}

pub fn source_artifact_hint(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> String {
    let candidates = source_candidates(kind, ctx).unwrap_or_default();
    hint_from_candidates(name, &candidates).unwrap_or_else(|| format!("See 'cmx search {name}'."))
}

fn installed_candidates(
    kind: Option<ArtifactKind>,
    ctx: &AppContext<'_>,
) -> crate::error::Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for platform in Platform::ALL {
        let paths = ctx.paths.with_platform(platform);
        for scope in InstallScope::ALL {
            let lock = lockfile::load(scope, ctx.fs, &paths)?;
            for (name, entry) in lock.packages {
                if kind.is_none_or(|expected| entry.artifact_type == expected) {
                    names.insert(name);
                }
            }
        }
    }
    Ok(names)
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
    use super::{installed_artifact_hint, levenshtein};
    use crate::lockfile;
    use crate::test_support::{TestContext, sample_lock_entry};
    use crate::types::{ArtifactKind, InstallScope, LockFile};
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

        let hint = installed_artifact_hint("focus-skll", Some(ArtifactKind::Skill), &t.ctx());
        assert_eq!(hint, "Did you mean 'focus-skill'?");
    }
}
