//! Shared display formatting utilities, a submodule of
//! `cmx/src/display/mod.rs`.

use std::fmt;

use crate::diff::{FileChange, FileStatus};

/// Format an optional version string, falling back to `"unversioned"`.
pub(crate) fn version_label(version: Option<&str>) -> &str {
    version.unwrap_or("unversioned")
}

/// The standard plan-mode footer: reminds the user how to actually apply the
/// change plan just shown.
pub(crate) const APPLY_HINT: &str = "Re-run with --apply to make these changes.";

/// Format `count` with an irregular singular/plural word pair, e.g.
/// `count_label(1, "copy", "copies")` -> `"1 copy"`. For the regular `"s"`
/// suffix, see `crate::table::plural_s`.
pub(crate) fn count_label(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

/// Count (modified, added, deleted) across a slice of [`FileChange`]s.
pub(super) fn change_counts(changes: &[FileChange]) -> (usize, usize, usize) {
    changes
        .iter()
        .fold((0, 0, 0), |(modified, added, deleted), change| match change.status {
            FileStatus::Modified => (modified + 1, added, deleted),
            FileStatus::OnlyInInstalled => (modified, added, deleted + 1),
            FileStatus::OnlyInSource => (modified, added + 1, deleted),
        })
}

/// Write one line per changed file into `f`, rooted at `target_root`.
///
/// `changes` must be plan-oriented (as [`crate::diff::file_changes_between`]
/// returns them): `added` is what applying the plan adds to `target_root` and
/// `removed` is what it takes away, so `+`/`−` here mean what they mean in
/// `diff(1)`. These lines are what a user reads to decide whether to re-run with
/// `--apply`, so an inverted count doesn't just misinform — it makes an additive
/// plan look destructive.
pub(super) fn write_change_lines(
    f: &mut fmt::Formatter<'_>,
    target_root: &std::path::Path,
    changes: &[FileChange],
) -> fmt::Result {
    for change in changes {
        let detail = match change.status {
            FileStatus::Modified => format!("modified (+{} -{})", change.added, change.removed),
            FileStatus::OnlyInInstalled => format!("deleted (-{})", change.removed),
            FileStatus::OnlyInSource => format!("added (+{})", change.added),
        };
        writeln!(f, "    {}  {detail}", target_root.join(&change.path).display())?;
    }
    Ok(())
}

/// Write one "Discarding local modification: <path>" line per distinct path in
/// `paths`, in first-seen order deduplicated by path.
pub(super) fn write_discarded_paths<'a>(
    f: &mut fmt::Formatter<'_>,
    paths: impl IntoIterator<Item = &'a std::path::PathBuf>,
) -> fmt::Result {
    let mut seen = std::collections::BTreeSet::<&std::path::PathBuf>::new();
    for path in paths {
        if seen.insert(path) {
            writeln!(f, "Discarding local modification: {}", path.display())?;
        }
    }
    Ok(())
}

pub(super) fn write_each<T: std::fmt::Display>(
    f: &mut std::fmt::Formatter<'_>,
    items: &[T],
) -> std::fmt::Result {
    for item in items {
        write!(f, "{item}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_each_concatenates_display_items() {
        struct Wrapper(Vec<String>);
        impl std::fmt::Display for Wrapper {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write_each(f, &self.0)
            }
        }
        let w = Wrapper(vec!["hello".to_string(), " world".to_string()]);
        assert_eq!(w.to_string(), "hello world");
    }

    #[test]
    fn write_each_empty_slice_emits_nothing() {
        struct Wrapper(Vec<String>);
        impl std::fmt::Display for Wrapper {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write_each(f, &self.0)
            }
        }
        let w = Wrapper(vec![]);
        assert_eq!(w.to_string(), "");
    }
}
