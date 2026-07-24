//! Shared display formatting utilities, a submodule of
//! `cmx/src/display/mod.rs`.

use std::fmt;

use crate::diff::{FileChange, FileStatus};
use crate::table::{empty_state, render_table};

/// Format an optional version string, falling back to `"unversioned"`.
pub(super) fn version_label(version: Option<&str>) -> &str {
    version.unwrap_or("unversioned")
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
pub(super) fn write_change_lines(
    f: &mut fmt::Formatter<'_>,
    target_root: &std::path::Path,
    changes: &[FileChange],
) -> fmt::Result {
    for change in changes {
        let detail = match change.status {
            FileStatus::Modified => format!("modified (+{} -{})", change.added, change.removed),
            FileStatus::OnlyInInstalled => format!("deleted (-{})", change.added),
            FileStatus::OnlyInSource => format!("added (+{})", change.removed),
        };
        writeln!(f, "    {}  {detail}", target_root.join(&change.path).display())?;
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

pub(super) fn table_or_empty(
    empty_msg: &str,
    headers: Vec<&'static str>,
    padded_cols: usize,
    rows: Vec<Vec<String>>,
) -> String {
    if rows.is_empty() {
        empty_state(empty_msg)
    } else {
        render_table(headers, padded_cols, rows)
    }
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

    #[test]
    fn table_or_empty_returns_newline_terminated_message_when_no_rows() {
        let result = table_or_empty("Nothing here.", vec!["Name"], 1, vec![]);
        assert_eq!(result, "Nothing here.\n");
    }

    #[test]
    fn table_or_empty_returns_table_containing_data_when_rows_present() {
        let result =
            table_or_empty("Nothing here.", vec!["Name"], 1, vec![vec!["my-item".to_string()]]);
        assert!(result.contains("my-item"));
    }
}
