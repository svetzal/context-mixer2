//! Honouring the pinned revision: which tree of the atlas cmv reads
//! records from and runs validators in (see `CMV.md`, design decision 5).
//!
//! When the manifest records a `revision`, the resolved root is a git
//! checkout, and its `HEAD` differs from the pin, cmv verifies against the
//! **pinned tree**: it materializes that commit into the run's scratch
//! directory (`git archive` into a tar, then `tar -xf`, both through the
//! [`ProcessRunner`] gateway — never through binary stdout, which the runner
//! decodes lossily) and validators run there. When `HEAD` equals the pin,
//! there is no pin, or the root is not a git checkout, the root itself is
//! used. [`PinPolicy::AtHead`] (`--at-head`) skips the pin and verifies the
//! working tree, for validator authors iterating on an atlas.
//!
//! Whether the atlas has *moved* (`HEAD` differs from the pin) is
//! reported but never affects the exit code: cmv never re-pins, and
//! recompiling is cmf's decision.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cmx_core::gateway::Filesystem;
use intent_atlas::manifest::git_head_commit;
use serde::Serialize;

use crate::dispatch::Trees;
use crate::process::{ProcessOutcome, ProcessRequest, ProcessRunner};
use crate::resolve::{Resolution, ResolvedBy};

/// Whether to honour the manifest's pinned revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinPolicy {
    /// Verify against the pinned tree when `HEAD` has moved past it.
    Pinned,
    /// Verify against the working tree regardless of the pin (`--at-head`).
    AtHead,
}

impl PinPolicy {
    /// Convert from the raw `--at-head` flag, exactly once, at the CLI
    /// boundary.
    pub fn from_flag(at_head: bool) -> Self {
        if at_head {
            PinPolicy::AtHead
        } else {
            PinPolicy::Pinned
        }
    }
}

/// Which tree the verdicts were produced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedAgainst {
    /// The revision the manifest pinned (materialized, or checked out as
    /// `HEAD`).
    Pinned,
    /// The working tree, because there was no pin to honour or `--at-head`
    /// asked for it.
    Head,
}

/// The tree cmv verifies against, and how it relates to the pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// The tree records are read from and validators run in.
    pub root: PathBuf,
    /// The atlas's own working tree, when `root` is a materialized
    /// pinned tree rather than the working tree itself.
    pub working_tree: Option<PathBuf>,
    /// The commit the working tree's `HEAD` points at, when it is a git
    /// checkout.
    pub head_revision: Option<String>,
    /// The revision the manifest recorded, if any.
    pub pinned_revision: Option<String>,
    /// Which tree `root` is.
    pub verified_against: VerifiedAgainst,
}

impl Checkout {
    /// Whether `HEAD` differs from the pin. `false` when either is unknown.
    pub fn moved(&self) -> bool {
        match (&self.head_revision, &self.pinned_revision) {
            (Some(head), Some(pinned)) => head != pinned,
            _ => false,
        }
    }

    /// The trees a run reads from: `root`, plus the working tree when `root`
    /// is a materialized pinned tree.
    pub fn trees(&self) -> Trees<'_> {
        Trees {
            verified: &self.root,
            working: self.working_tree.as_deref(),
        }
    }
}

/// How long `git archive` and `tar` may each take before cmv gives up.
pub const MATERIALIZE_TIMEOUT: Duration = Duration::from_secs(120);

/// Decide which tree to verify against and, when that is a pinned revision
/// the working tree has moved past, materialize it under `scratch`.
///
/// Fails when the pinned revision cannot be archived (typically because the
/// local checkout has not fetched it), naming the revision and the remedy.
pub fn materialize(
    resolution: &Resolution,
    pinned: Option<&str>,
    policy: PinPolicy,
    scratch: &Path,
    fs: &dyn Filesystem,
    runner: &dyn ProcessRunner,
) -> Result<Checkout> {
    let head = git_head_commit(&resolution.path, fs);
    let at_root = |verified_against| Checkout {
        root: resolution.path.clone(),
        working_tree: None,
        head_revision: head.clone(),
        pinned_revision: pinned.map(str::to_string),
        verified_against,
    };
    match (policy, &head, pinned) {
        (PinPolicy::AtHead, _, _) => Ok(at_root(VerifiedAgainst::Head)),
        (PinPolicy::Pinned, Some(head), Some(pinned)) if head == pinned => {
            Ok(at_root(VerifiedAgainst::Pinned))
        }
        (PinPolicy::Pinned, Some(_), Some(pinned)) => {
            let root = materialize_pinned_tree(resolution, pinned, scratch, fs, runner)?;
            Ok(Checkout {
                root,
                working_tree: Some(resolution.path.clone()),
                head_revision: head,
                pinned_revision: Some(pinned.to_string()),
                verified_against: VerifiedAgainst::Pinned,
            })
        }
        (PinPolicy::Pinned, None, _) | (PinPolicy::Pinned, _, None) => {
            Ok(at_root(VerifiedAgainst::Head))
        }
    }
}

/// `git archive` the pinned commit into `<scratch>/kb.tar` and unpack it into
/// `<scratch>/kb`, which is returned.
fn materialize_pinned_tree(
    resolution: &Resolution,
    revision: &str,
    scratch: &Path,
    fs: &dyn Filesystem,
    runner: &dyn ProcessRunner,
) -> Result<PathBuf> {
    // `git -C` and the working directory are both the checkout, so a root
    // given relative to cmv's working directory must be made absolute first.
    let root = fs
        .canonicalize(&resolution.path)
        .with_context(|| format!("could not resolve atlas {}", resolution.path.display()))?;
    let tar = scratch.join("kb.tar");
    let tree = scratch.join("kb");
    let archive = ProcessRequest {
        program: PathBuf::from("git"),
        args: [
            OsString::from("-C"),
            root.as_os_str().to_owned(),
            OsString::from("archive"),
            OsString::from("--format=tar"),
            OsString::from("-o"),
            tar.as_os_str().to_owned(),
            OsString::from(revision),
        ]
        .to_vec(),
        cwd: root.clone(),
        timeout: MATERIALIZE_TIMEOUT,
    };
    if let Err(reason) = succeeded(runner.run(&archive), "git archive") {
        bail!(
            "revision {revision} pinned by the manifest is not reachable in the atlas at {} ({reason}); {} to fetch it, or pass --at-head to verify the working tree instead",
            root.display(),
            fetch_remedy(resolution)
        );
    }
    fs.create_dir_all(&tree)
        .with_context(|| format!("could not create {}", tree.display()))?;
    let extract = ProcessRequest {
        program: PathBuf::from("tar"),
        args: [
            OsString::from("-xf"),
            tar.as_os_str().to_owned(),
            OsString::from("-C"),
            tree.as_os_str().to_owned(),
        ]
        .to_vec(),
        cwd: scratch.to_path_buf(),
        timeout: MATERIALIZE_TIMEOUT,
    };
    if let Err(reason) = succeeded(runner.run(&extract), "tar") {
        bail!("could not unpack the pinned revision {revision}: {reason}");
    }
    Ok(tree)
}

/// The command that fetches a missing revision: through cmx when the source
/// is registered, plain git otherwise.
fn fetch_remedy(resolution: &Resolution) -> String {
    match (&resolution.source, resolution.resolved_by) {
        (Some(name), ResolvedBy::Source) => format!("run `cmx source update {name}`"),
        _ => "run `git fetch` there".to_string(),
    }
}

/// `Ok` when the process exited 0, else a one-line description of how it
/// ended.
fn succeeded(outcome: ProcessOutcome, what: &str) -> std::result::Result<(), String> {
    match outcome {
        ProcessOutcome::Exited { code: Some(0), .. } => Ok(()),
        ProcessOutcome::Exited { code, stderr, .. } => {
            let status = code.map_or_else(
                || "was killed by a signal".to_string(),
                |code| format!("exited {code}"),
            );
            Err(match stderr.lines().map(str::trim).find(|line| !line.is_empty()) {
                Some(line) => format!("{what} {status}: {line}"),
                None => format!("{what} {status}"),
            })
        }
        ProcessOutcome::TimedOut => {
            Err(format!("{what} timed out after {}s", MATERIALIZE_TIMEOUT.as_secs()))
        }
        ProcessOutcome::FailedToStart { reason } => {
            Err(format!("could not start {what}: {reason}"))
        }
    }
}

/// The `atlas` block of every report: where the records came from,
/// how cmv found them, and how the tree it verified relates to the pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AtlasReport {
    /// The atlas root as resolved.
    pub path: PathBuf,
    /// Which step of the resolution order produced `path`.
    pub resolved_by: ResolvedBy,
    /// The cmx source name the manifest recorded, if any.
    pub source: Option<String>,
    /// The revision the manifest recorded, if any.
    pub pinned_revision: Option<String>,
    /// The working tree's `HEAD`, when it is a git checkout.
    pub head_revision: Option<String>,
    /// Which tree the verdicts came from.
    pub verified_against: VerifiedAgainst,
    /// Whether `HEAD` differs from the pin.
    pub moved: bool,
}

impl AtlasReport {
    /// Combine where the atlas was found with which tree was used.
    pub fn new(resolution: &Resolution, checkout: &Checkout) -> Self {
        Self {
            path: resolution.path.clone(),
            resolved_by: resolution.resolved_by,
            source: resolution.source.clone(),
            pinned_revision: checkout.pinned_revision.clone(),
            head_revision: checkout.head_revision.clone(),
            verified_against: checkout.verified_against,
            moved: checkout.moved(),
        }
    }
}

/// The first twelve characters of a revision, for human output.
pub fn short_revision(revision: &str) -> &str {
    revision.get(..12).unwrap_or(revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{FakeProcessRunner, exited};
    use cmx_core::gateway::fakes::FakeFilesystem;

    const PIN: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const HEAD: &str = "ffffffffffffffffffffffffffffffffffffffff";
    const KB: &str = "/kb";
    const SCRATCH: &str = "/scratch";

    fn resolution(resolved_by: ResolvedBy, source: Option<&str>) -> Resolution {
        Resolution {
            path: PathBuf::from(KB),
            resolved_by,
            source: source.map(str::to_string),
            warning: None,
        }
    }

    fn checkout_at(fs: &FakeFilesystem, head: &str) {
        fs.add_file(format!("{KB}/.git/HEAD"), format!("{head}\n"));
    }

    fn archive_request() -> ProcessRequest {
        ProcessRequest {
            program: PathBuf::from("git"),
            args: [
                "-C",
                KB,
                "archive",
                "--format=tar",
                "-o",
                "/scratch/kb.tar",
                PIN,
            ]
            .iter()
            .map(OsString::from)
            .collect(),
            cwd: PathBuf::from(KB),
            timeout: MATERIALIZE_TIMEOUT,
        }
    }

    fn extract_request() -> ProcessRequest {
        ProcessRequest {
            program: PathBuf::from("tar"),
            args: ["-xf", "/scratch/kb.tar", "-C", "/scratch/kb"]
                .iter()
                .map(OsString::from)
                .collect(),
            cwd: PathBuf::from(SCRATCH),
            timeout: MATERIALIZE_TIMEOUT,
        }
    }

    fn materialize_with(
        fs: &FakeFilesystem,
        resolution: &Resolution,
        pinned: Option<&str>,
        policy: PinPolicy,
        runner: &FakeProcessRunner,
    ) -> Result<Checkout> {
        materialize(resolution, pinned, policy, Path::new(SCRATCH), fs, runner)
    }

    #[test]
    fn moved_head_materializes_the_pinned_tree_with_git_archive_then_tar() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new()
            .script("git", exited(0, "", ""))
            .script("tar", exited(0, "", ""));
        let resolution = resolution(ResolvedBy::Source, Some("guidelines"));
        let checkout =
            materialize_with(&fs, &resolution, Some(PIN), PinPolicy::Pinned, &runner).unwrap();
        assert_eq!(
            checkout,
            Checkout {
                root: PathBuf::from("/scratch/kb"),
                working_tree: Some(PathBuf::from(KB)),
                head_revision: Some(HEAD.to_string()),
                pinned_revision: Some(PIN.to_string()),
                verified_against: VerifiedAgainst::Pinned,
            }
        );
        assert!(checkout.moved());
        assert_eq!(runner.calls(), vec![archive_request(), extract_request()]);
        assert!(
            fs.is_dir(Path::new("/scratch/kb")),
            "the unpack directory is created for tar -C"
        );
    }

    #[test]
    fn unreachable_revision_names_the_pin_and_the_cmx_remedy() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new().script(
            "git",
            exited(
                128,
                "",
                "fatal: not a valid object name: a1b2c3d4e5f60718293a4b5c6d7e8f9012345678\n",
            ),
        );
        let resolution = resolution(ResolvedBy::Source, Some("guidelines"));
        let error = materialize_with(&fs, &resolution, Some(PIN), PinPolicy::Pinned, &runner)
            .unwrap_err()
            .to_string();
        assert!(error.contains(PIN), "{error}");
        assert!(
            error.contains("git archive exited 128: fatal: not a valid object name"),
            "{error}"
        );
        assert!(error.contains("run `cmx source update guidelines`"), "{error}");
        assert!(error.contains("--at-head"), "{error}");
        assert_eq!(runner.calls().len(), 1, "tar is not attempted");
    }

    #[test]
    fn unreachable_revision_suggests_git_fetch_without_a_registered_source() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new().script("git", exited(128, "", ""));
        for resolution in [
            resolution(ResolvedBy::Path, Some("guidelines")),
            resolution(ResolvedBy::Override, None),
        ] {
            let error = materialize_with(&fs, &resolution, Some(PIN), PinPolicy::Pinned, &runner)
                .unwrap_err()
                .to_string();
            assert!(error.contains("run `git fetch` there"), "{error}");
            assert!(!error.contains("cmx source update"), "{error}");
        }
    }

    #[test]
    fn missing_git_executable_is_reported_as_such() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new();
        let error = materialize_with(
            &fs,
            &resolution(ResolvedBy::Path, None),
            Some(PIN),
            PinPolicy::Pinned,
            &runner,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("could not start git archive"), "{error}");
    }

    #[test]
    fn failed_unpack_is_an_error() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new()
            .script("git", exited(0, "", ""))
            .script("tar", exited(1, "", "tar: Damaged tar archive\n"));
        let error = materialize_with(
            &fs,
            &resolution(ResolvedBy::Path, None),
            Some(PIN),
            PinPolicy::Pinned,
            &runner,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("could not unpack the pinned revision"), "{error}");
        assert!(error.contains("tar exited 1: tar: Damaged tar archive"), "{error}");
    }

    #[test]
    fn head_equal_to_the_pin_uses_the_root_as_the_pinned_tree() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, PIN);
        let runner = FakeProcessRunner::new();
        let checkout = materialize_with(
            &fs,
            &resolution(ResolvedBy::Path, None),
            Some(PIN),
            PinPolicy::Pinned,
            &runner,
        )
        .unwrap();
        assert_eq!(checkout.root, PathBuf::from(KB));
        assert_eq!(checkout.working_tree, None);
        assert_eq!(checkout.verified_against, VerifiedAgainst::Pinned);
        assert!(!checkout.moved());
        assert!(runner.calls().is_empty(), "nothing is archived");
    }

    #[test]
    fn at_head_bypasses_the_pin_and_says_so() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new();
        let checkout = materialize_with(
            &fs,
            &resolution(ResolvedBy::Path, None),
            Some(PIN),
            PinPolicy::AtHead,
            &runner,
        )
        .unwrap();
        assert_eq!(checkout.root, PathBuf::from(KB));
        assert_eq!(checkout.working_tree, None);
        assert_eq!(checkout.verified_against, VerifiedAgainst::Head);
        assert_eq!(checkout.head_revision.as_deref(), Some(HEAD));
        assert_eq!(checkout.pinned_revision.as_deref(), Some(PIN));
        assert!(checkout.moved(), "moved is still reported");
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn no_pin_or_no_git_checkout_verifies_the_root_at_head() {
        let runner = FakeProcessRunner::new();
        let resolution = resolution(ResolvedBy::Path, None);

        let git_without_pin = FakeFilesystem::new();
        checkout_at(&git_without_pin, HEAD);
        let checkout =
            materialize_with(&git_without_pin, &resolution, None, PinPolicy::Pinned, &runner)
                .unwrap();
        assert_eq!(checkout.verified_against, VerifiedAgainst::Head);
        assert_eq!(checkout.head_revision.as_deref(), Some(HEAD));
        assert!(!checkout.moved());

        let plain_directory = FakeFilesystem::new();
        let checkout =
            materialize_with(&plain_directory, &resolution, Some(PIN), PinPolicy::Pinned, &runner)
                .unwrap();
        assert_eq!(checkout.root, PathBuf::from(KB));
        assert_eq!(checkout.verified_against, VerifiedAgainst::Head);
        assert_eq!(checkout.head_revision, None);
        assert_eq!(checkout.pinned_revision.as_deref(), Some(PIN));
        assert!(!checkout.moved(), "an unknown HEAD cannot have moved");
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn report_combines_resolution_and_checkout() {
        let fs = FakeFilesystem::new();
        checkout_at(&fs, HEAD);
        let runner = FakeProcessRunner::new();
        let resolution = resolution(ResolvedBy::Source, Some("guidelines"));
        let checkout =
            materialize_with(&fs, &resolution, Some(PIN), PinPolicy::AtHead, &runner).unwrap();
        let report = AtlasReport::new(&resolution, &checkout);
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({
                "path": "/kb",
                "resolved_by": "source",
                "source": "guidelines",
                "pinned_revision": PIN,
                "head_revision": HEAD,
                "verified_against": "head",
                "moved": true,
            })
        );
    }

    #[test]
    fn short_revision_takes_twelve_characters_or_everything() {
        assert_eq!(short_revision(PIN), "a1b2c3d4e5f6");
        assert_eq!(short_revision("abc"), "abc");
    }
}
