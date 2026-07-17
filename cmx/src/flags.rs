//! Intent-revealing flag types for the `cmx` CLI shell.
//!
//! These enums replace positional `bool` parameters at call sites, making the
//! purpose of each flag legible without reading the full function signature.
//!
//! Conversion from raw `bool` (as received from clap) happens exactly once at
//! the dispatch boundary in `cmx/src/dispatch/` and `cmx/src/main.rs`; only
//! those sites should call the `from_flag` constructors.

/// Whether a mutating command should execute changes or only preview them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunMode {
    /// Execute the changes for real.
    Apply,
    /// Compute and display the plan without executing it.
    Plan,
}

impl RunMode {
    /// Convert from a raw `--apply` flag (`true` = apply, `false` = plan).
    pub fn from_flag(apply: bool) -> Self {
        if apply { RunMode::Apply } else { RunMode::Plan }
    }

    /// `true` when changes should be executed.
    pub fn is_apply(self) -> bool {
        self == RunMode::Apply
    }
}

/// Whether to force-overwrite a locally modified artifact.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Force {
    Yes,
    No,
}

impl Force {
    /// Convert from a raw `--force` flag.
    pub fn from_flag(force: bool) -> Self {
        if force { Force::Yes } else { Force::No }
    }

    /// `true` when the force override is active.
    pub fn is_yes(self) -> bool {
        self == Force::Yes
    }
}

/// Whether to also uninstall artifacts when deleting a set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purge {
    Yes,
    No,
}

impl Purge {
    /// Convert from a raw `--purge` flag.
    pub fn from_flag(purge: bool) -> Self {
        if purge { Purge::Yes } else { Purge::No }
    }

    /// `true` when the purge is requested.
    pub fn is_yes(self) -> bool {
        self == Purge::Yes
    }
}

/// Whether to operate on all available artifacts or only the named ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Selection {
    /// Operate on every artifact matching the criteria.
    All,
    /// Operate only on the explicitly named artifacts.
    Named,
}

impl Selection {
    /// Convert from a raw `--all` flag.
    pub fn from_flag(all: bool) -> Self {
        if all {
            Selection::All
        } else {
            Selection::Named
        }
    }

    /// `true` when all artifacts should be included.
    pub fn is_all(self) -> bool {
        self == Selection::All
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_mode_from_flag_round_trips() {
        assert_eq!(RunMode::from_flag(true), RunMode::Apply);
        assert_eq!(RunMode::from_flag(false), RunMode::Plan);
        assert!(RunMode::Apply.is_apply());
        assert!(!RunMode::Plan.is_apply());
    }

    #[test]
    fn force_from_flag_round_trips() {
        assert_eq!(Force::from_flag(true), Force::Yes);
        assert_eq!(Force::from_flag(false), Force::No);
        assert!(Force::Yes.is_yes());
        assert!(!Force::No.is_yes());
    }

    #[test]
    fn purge_from_flag_round_trips() {
        assert_eq!(Purge::from_flag(true), Purge::Yes);
        assert_eq!(Purge::from_flag(false), Purge::No);
        assert!(Purge::Yes.is_yes());
        assert!(!Purge::No.is_yes());
    }

    #[test]
    fn selection_from_flag_round_trips() {
        assert_eq!(Selection::from_flag(true), Selection::All);
        assert_eq!(Selection::from_flag(false), Selection::Named);
        assert!(Selection::All.is_all());
        assert!(!Selection::Named.is_all());
    }
}
