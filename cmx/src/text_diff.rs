//! General-purpose line-oriented LCS text differ.
//!
//! This module is self-contained with no coupling to cmx's artifact model.
//! It provides `split_lines`, `lcs_ops`, and `render_hunks` for computing and
//! rendering a compact unified-style diff between two text strings.

use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Equal,
    Del,
    Ins,
}

pub(crate) fn split_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        Vec::new()
    } else {
        s.lines().collect()
    }
}

/// Longest-common-subsequence line diff. Returns ops in order: `Del` lines come
/// from `old` (the source/`−` side), `Ins` from `new` (the installed/`+` side).
/// Falls back to a whole-file replace for pathologically large inputs to bound
/// the O(n·m) table.
pub(crate) fn lcs_ops<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Op, &'a str)> {
    let (n, m) = (old.len(), new.len());
    if n.saturating_mul(m) > 4_000_000 {
        let mut ops = Vec::with_capacity(n + m);
        ops.extend(old.iter().map(|l| (Op::Del, *l)));
        ops.extend(new.iter().map(|l| (Op::Ins, *l)));
        return ops;
    }

    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[idx(i, j)] = if old[i] == new[j] {
                dp[idx(i + 1, j + 1)] + 1
            } else {
                dp[idx(i + 1, j)].max(dp[idx(i, j + 1)])
            };
        }
    }

    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            ops.push((Op::Equal, old[i]));
            i += 1;
            j += 1;
        } else if dp[idx(i + 1, j)] >= dp[idx(i, j + 1)] {
            ops.push((Op::Del, old[i]));
            i += 1;
        } else {
            ops.push((Op::Ins, new[j]));
            j += 1;
        }
    }
    while i < n {
        ops.push((Op::Del, old[i]));
        i += 1;
    }
    while j < m {
        ops.push((Op::Ins, new[j]));
        j += 1;
    }
    ops
}

/// Render ops as a compact diff: changed lines (`-`/`+`) with `context` lines of
/// surrounding context; runs of unchanged lines outside the context window
/// collapse to a `⋮ (N unchanged lines)` marker.
pub(crate) fn render_hunks(ops: &[(Op, &str)], context: usize) -> String {
    let n = ops.len();
    let mut keep = vec![false; n];
    let mut any_change = false;
    for (i, (op, _)) in ops.iter().enumerate() {
        if *op != Op::Equal {
            any_change = true;
            let lo = i.saturating_sub(context);
            let hi = (i + context + 1).min(n);
            for slot in keep.iter_mut().take(hi).skip(lo) {
                *slot = true;
            }
        }
    }
    if !any_change {
        return String::new();
    }

    let mut out = String::new();
    let mut i = 0;
    while i < n {
        if !keep[i] {
            let start = i;
            while i < n && !keep[i] {
                i += 1;
            }
            let skipped = i - start;
            let plural = if skipped == 1 { "" } else { "s" };
            let _ = writeln!(out, "     ⋮ ({skipped} unchanged line{plural})");
            continue;
        }
        let (op, text) = ops[i];
        let prefix = match op {
            Op::Equal => " ",
            Op::Del => "-",
            Op::Ins => "+",
        };
        let _ = writeln!(out, "  {prefix} {text}");
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcs_ops_marks_inserts_and_deletes() {
        let old = vec!["a", "b", "c"];
        let new = vec!["a", "x", "c"];
        let ops = lcs_ops(&old, &new);
        let added = ops.iter().filter(|(o, _)| *o == Op::Ins).count();
        let removed = ops.iter().filter(|(o, _)| *o == Op::Del).count();
        assert_eq!((added, removed), (1, 1), "one line replaced");
    }

    #[test]
    fn lcs_ops_identical_is_all_equal() {
        let v = vec!["a", "b"];
        let ops = lcs_ops(&v, &v);
        assert!(ops.iter().all(|(o, _)| *o == Op::Equal));
    }

    #[test]
    fn render_hunks_collapses_unchanged_runs() {
        // 10 equal lines, then one changed line.
        let mut old: Vec<&str> = (0..10).map(|_| "same").collect();
        old.push("old-tail");
        let mut new: Vec<&str> = (0..10).map(|_| "same").collect();
        new.push("new-tail");
        let ops = lcs_ops(&old, &new);
        let out = render_hunks(&ops, 3);
        assert!(out.contains("⋮"), "collapses the long unchanged run: {out}");
        assert!(out.contains("- old-tail"), "shows the removed line: {out}");
        assert!(out.contains("+ new-tail"), "shows the added line: {out}");
        assert!(!out.contains("⋮ (0 unchanged"), "no zero-length markers: {out}");
    }

    #[test]
    fn render_hunks_empty_for_no_changes() {
        let v = vec!["a", "b"];
        let ops = lcs_ops(&v, &v);
        assert!(render_hunks(&ops, 3).is_empty());
    }
}
