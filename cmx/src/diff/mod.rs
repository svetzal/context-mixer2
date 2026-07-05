use anyhow::{Result, bail};
use std::path::PathBuf;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::platform::Platform;
use crate::source_iter;
use crate::types::ArtifactKind;

#[cfg(feature = "llm")]
mod analyze;
mod discovery;
mod reconcile;
mod structural;

use discovery::{discover_copies, evaluate_copies, representative_platform};
use reconcile::{focus_lock_state, reconciliations};

// ---------------------------------------------------------------------------
// Result types (public surface — unchanged from before)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DiffOutput {
    pub artifact_name: String,
    pub kind: ArtifactKind,
    pub is_up_to_date: bool,
    /// Where the installed copy lives (the side `+` lines come from).
    pub installed_path: PathBuf,
    pub installed_version: Option<String>,
    /// `true` when the installed copy was edited after install (its bytes no
    /// longer match the lock's recorded checksum).
    pub installed_locally_edited: bool,
    /// Where the source copy lives (the side `−` lines come from).
    pub source_path: PathBuf,
    pub source_version: Option<String>,
    pub source_name: String,
    /// Per-file summary of what differs, so the direction of each change is
    /// legible without reading the whole diff.
    pub file_changes: Vec<FileChange>,
    pub diff_text: Option<String>,
    pub analysis: Option<String>,
    /// A one-line note shown in place of `analysis` when the LLM summary is
    /// unavailable (gateway not configured, LLM error, or a lean build without
    /// the `llm` feature). Mutually exclusive with `analysis`.
    pub analysis_note: Option<String>,
    /// The reconciliation directions to offer — both ways, since `diff` can't
    /// know which side is authoritative.
    pub reconciliations: Vec<Reconciliation>,
    /// When `true`, render the full line-by-line unified diff; otherwise the
    /// output stays compact (summary + analysis) with a hint to pass `--full`.
    pub show_full: bool,
    /// Every installed copy and how it compares to the source. With more than
    /// one entry the display shows a per-platform matrix; the detailed diff and
    /// analysis below focus the copy flagged `is_focus`.
    pub copies: Vec<CopyStatus>,
    /// Concrete name for the focused (changed) side — the platform whose copy is
    /// being shown, e.g. `codex`. Paired with `source_name` (e.g. `home`) these
    /// are the only two labels the output (and the LLM summary) uses, so the
    /// reader never has to map "installed"/"source" onto a real copy.
    pub changed_label: String,
}

/// One installed copy of the artifact and how it compares to the source.
#[derive(Debug, Clone)]
pub struct CopyStatus {
    /// The platforms whose install directory resolves to this copy (a shared
    /// `.agents/skills` copy lists several).
    pub platforms: Vec<Platform>,
    pub path: PathBuf,
    /// `true` when this copy is byte-identical to the source.
    pub matches: bool,
    pub added: usize,
    pub removed: usize,
    /// `true` for the copy whose detailed diff/analysis is shown below.
    pub is_focus: bool,
}

/// How one file differs between the source and installed copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// Present on both sides with differing content.
    Modified,
    /// Present only in the installed copy (added locally).
    OnlyInInstalled,
    /// Present only in the source copy (removed locally).
    OnlyInSource,
}

/// One file's change summary. `added` counts lines present only in the installed
/// copy (`+`); `removed` counts lines present only in the source copy (`−`).
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub status: FileStatus,
    pub added: usize,
    pub removed: usize,
}

/// One way to reconcile the difference: a human-readable direction plus the
/// exact command, with an optional caveat.
#[derive(Debug, Clone)]
pub struct Reconciliation {
    pub description: String,
    pub command: String,
    pub note: Option<String>,
}

/// Names the two copies being compared by their concrete identities, so every
/// downstream function speaks the same language (never "source"/"installed").
pub(crate) struct FocusedComparison<'a> {
    pub(crate) name: &'a str,
    pub(crate) kind: ArtifactKind,
    pub(crate) source_name: &'a str,
    pub(crate) changed_label: &'a str,
    /// Read only by the LLM prompt (`analyze::analyze_focus`); unused in lean
    /// builds, which never construct a prompt.
    #[cfg_attr(not(feature = "llm"), allow(dead_code))]
    pub(crate) source_version: Option<&'a str>,
    /// Read only by the LLM prompt (`analyze::analyze_focus`); unused in lean
    /// builds, which never construct a prompt.
    #[cfg_attr(not(feature = "llm"), allow(dead_code))]
    pub(crate) changed_version: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn diff(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<DiffOutput> {
    let mut output = gather_diff_with(name, kind, ctx)?;
    output.show_full = full;
    Ok(output)
}

// ---------------------------------------------------------------------------
// Gather (no println!)
// ---------------------------------------------------------------------------

pub(crate) fn gather_diff_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<DiffOutput> {
    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources_with(name, kind, ctx)?;
    let source_checksum = checksum::checksum_artifact(&source_path, kind, ctx.fs)?;

    // Discover every installed copy (skills can live on several platforms; a copy
    // matching source on one platform says nothing about the others).
    let (raw_copies, scope) = discover_copies(name, kind, ctx)?;
    if raw_copies.is_empty() {
        bail!("No installed {kind} named '{name}' found on disk.");
    }

    // Compare each copy to the source; build the per-copy diff for differing ones.
    let evals =
        evaluate_copies(raw_copies, kind, &source_checksum, &source_path, &source_name, ctx)?;

    // Focus the copy the user most likely means: the active platform's copy when
    // it differs, otherwise the first differing copy.
    let active = ctx.paths.platform;
    let focus = evals
        .iter()
        .position(|e| !e.matches && e.copy.platforms.contains(&active))
        .or_else(|| evals.iter().position(|e| !e.matches));

    let copies: Vec<CopyStatus> = evals
        .iter()
        .enumerate()
        .map(|(i, e)| CopyStatus {
            platforms: e.copy.platforms.clone(),
            path: e.copy.path.clone(),
            matches: e.matches,
            added: e.added,
            removed: e.removed,
            is_focus: Some(i) == focus,
        })
        .collect();

    // Every copy matches the source — nothing to reconcile anywhere.
    let Some(focus_idx) = focus else {
        return Ok(DiffOutput {
            artifact_name: name.to_string(),
            kind,
            is_up_to_date: true,
            installed_path: evals[0].copy.path.clone(),
            installed_version: None,
            installed_locally_edited: false,
            source_path,
            source_version,
            source_name,
            file_changes: Vec::new(),
            diff_text: None,
            analysis: None,
            analysis_note: None,
            reconciliations: Vec::new(),
            show_full: false,
            copies,
            changed_label: String::new(),
        });
    };

    let multi = copies.len() > 1;
    let managed = config::managed_platforms(ctx.fs, ctx.paths)?;
    let focus_platform =
        representative_platform(&evals[focus_idx].copy, active, managed.as_deref());
    // The two labels the whole output uses: `home`/<repo> on the `−` side, the
    // platform name on the `+` side.
    let changed_label = focus_platform.to_string();

    // Version + "locally edited" come from the focus copy's lock baseline.
    let focus_checksum = evals[focus_idx].copy.checksum.clone();
    let (installed_version, locally_modified) =
        focus_lock_state(name, &evals[focus_idx].copy, &focus_checksum, scope, ctx)?;

    let cmp = FocusedComparison {
        name,
        kind,
        source_name: &source_name,
        changed_label: &changed_label,
        source_version: source_version.as_deref(),
        changed_version: installed_version.as_deref(),
    };

    let reconciliations = reconciliations(&cmp, locally_modified, multi.then_some(focus_platform));

    let focus_eval = &evals[focus_idx];
    Ok(DiffOutput {
        artifact_name: name.to_string(),
        kind,
        is_up_to_date: false,
        installed_path: focus_eval.copy.path.clone(),
        installed_version,
        installed_locally_edited: locally_modified,
        source_path,
        source_version,
        source_name,
        file_changes: focus_eval.dir_diff.changes.clone(),
        diff_text: Some(focus_eval.dir_diff.unified.clone()),
        analysis: None,
        analysis_note: None,
        reconciliations,
        show_full: false,
        copies,
        changed_label,
    })
}

// ---------------------------------------------------------------------------
// LLM-backed analysis (optional, `llm`-feature only) — layered on top of the
// synchronous, LLM-free structural diff above.
// ---------------------------------------------------------------------------

/// Rebuild the [`FocusedComparison`] the diverged `output` was computed from
/// and ask the LLM to summarize its unified diff.
#[cfg(feature = "llm")]
async fn analyze(output: &DiffOutput, ctx: &AppContext<'_>) -> Result<String> {
    let cmp = FocusedComparison {
        name: &output.artifact_name,
        kind: output.kind,
        source_name: &output.source_name,
        changed_label: &output.changed_label,
        source_version: output.source_version.as_deref(),
        changed_version: output.installed_version.as_deref(),
    };
    analyze::analyze_focus(&cmp, output.diff_text.as_deref().unwrap_or(""), ctx).await
}

/// Like [`diff`], but also attempts an LLM summary of a diverged, compact-mode
/// diff. LLM failures degrade to [`DiffOutput::analysis_note`] rather than
/// failing the command — only a genuine diff-compute error propagates.
#[cfg(feature = "llm")]
pub async fn diff_with_analysis(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<DiffOutput> {
    let mut output = diff(name, kind, full, ctx)?;
    if !output.show_full && !output.is_up_to_date {
        match analyze(&output, ctx).await {
            Ok(prose) => output.analysis = Some(prose),
            Err(e) => output.analysis_note = Some(llm_unavailable_note(&e)),
        }
    }
    Ok(output)
}

/// One-line note shown in place of the analysis when the LLM summary failed
/// (gateway not configured, auth error, network error, ...). Never surfaces
/// the raw upstream error body.
#[cfg(feature = "llm")]
pub fn llm_unavailable_note(e: &anyhow::Error) -> String {
    format!(
        "note: LLM summary unavailable ({}). Fix the gateway (`cmx config gateway`, \
         `cmx config model`, or set OPENAI_API_KEY), or use --full for the plain diff.",
        crate::info::condense_error(e)
    )
}

/// One-line note shown in place of the analysis in a lean build (no `llm`
/// feature), so compact mode never claims LLM analysis it can't perform.
pub fn llm_lean_note() -> String {
    "note: LLM summary unavailable — this build lacks the llm feature. Use --full for the plain \
     line-by-line diff."
        .to_string()
}

fn find_in_sources_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, String, Option<String>)> {
    if let Some(sa) = source_iter::find_by_name_and_kind(name, kind, ctx)?.into_iter().next() {
        return Ok((sa.artifact.path, sa.source_name, sa.artifact.version));
    }
    bail!("No {kind} named '{name}' found in any registered source.");
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{
        TestContext, agent_content, install_agent_on_disk, make_lock_entry_versioned,
        save_lock_with_entry, setup_source_with_agent, test_paths,
    };
    use crate::types::{ArtifactKind, InstallScope};
    use chrono::Utc;

    // --- find_in_sources_with ---

    #[test]
    fn find_in_sources_locates_agent() {
        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let result = find_in_sources_with("my-agent", ArtifactKind::Agent, &ctx);
        assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    }

    #[test]
    fn find_in_sources_errors_when_not_found() {
        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "other-agent");

        let ctx = t.ctx();
        let result = find_in_sources_with("my-agent", ArtifactKind::Agent, &ctx);
        assert!(result.is_err());
    }

    // --- diff (top-level, synchronous, LLM-free) ---

    #[test]
    fn diff_with_reports_up_to_date_when_checksums_match() {
        let t = TestContext::new();
        let content = agent_content("my-agent", "A test agent");
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        t.fs.add_file("/sources/my-source/agents/my-agent.md", content.clone());
        install_agent_on_disk(&t.fs, &t.paths, "my-agent", &content, InstallScope::Global);
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        let output = diff("my-agent", ArtifactKind::Agent, false, &ctx).unwrap();
        assert!(output.is_up_to_date);
        assert!(output.reconciliations.is_empty(), "nothing to reconcile when in sync");
    }

    #[test]
    fn diff_succeeds_without_llm_when_checksums_differ() {
        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            "different installed content",
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        // `t.ctx()` carries `llm: None` — the plain, LLM-free diff must still
        // succeed and simply leave the analysis unset.
        let ctx = t.ctx();
        let result = diff("my-agent", ArtifactKind::Agent, false, &ctx);
        let output = result.expect("structural diff must not require an LLM");
        assert!(!output.is_up_to_date);
        assert!(output.analysis.is_none(), "plain diff() never attempts LLM analysis");
        assert!(output.analysis_note.is_none(), "plain diff() never sets a note either");
        assert!(output.diff_text.is_some(), "unified diff present");
        assert!(!output.file_changes.is_empty(), "file change recorded");
    }

    #[test]
    fn gather_diff_populates_paths_changes_and_reconciliations() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(
            &fs,
            &paths,
            "my-agent",
            "different installed content",
            InstallScope::Global,
        );
        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: None,
        };
        let output = gather_diff_with("my-agent", ArtifactKind::Agent, &ctx).unwrap();

        assert!(!output.is_up_to_date);
        assert!(output.analysis.is_none(), "gather never invokes the LLM itself");
        assert!(!output.file_changes.is_empty(), "file change recorded");
        assert!(output.diff_text.is_some(), "unified diff present");
        assert!(output.installed_path.ends_with("my-agent.md"), "installed path set");
        assert!(!output.reconciliations.is_empty(), "reconciliation directions offered");
        assert!(output.installed_locally_edited, "edited after install (checksum mismatch)");
    }

    #[test]
    fn gather_diff_skill_focuses_the_differing_platform() {
        use crate::test_support::{setup_source, skill_content};

        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source is the home; the Claude copy matches it, the Codex copy differs.
        let source = skill_content("the canonical skill");
        setup_source(&fs, &paths, "home", "/home-src");
        fs.add_file("/home-src/pf/SKILL.md", source.clone());
        let claude = paths.with_platform(crate::platform::Platform::Claude);
        fs.add_file(
            claude
                .install_dir(ArtifactKind::Skill, InstallScope::Global)
                .unwrap()
                .join("pf/SKILL.md"),
            source,
        );
        let codex = paths.with_platform(crate::platform::Platform::Codex);
        fs.add_file(
            codex
                .install_dir(ArtifactKind::Skill, InstallScope::Global)
                .unwrap()
                .join("pf/SKILL.md"),
            skill_content("the codex edits"),
        );
        // Scope the survey + suggestions to the two managed platforms.
        let config = crate::types::CmxConfig {
            platforms: vec![
                crate::platform::Platform::Claude,
                crate::platform::Platform::Codex,
            ],
            ..Default::default()
        };
        crate::config::save_config(&config, &fs, &paths).unwrap();

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: None,
        };
        // Active platform is Claude (the matching copy), yet diff must surface the
        // Codex divergence rather than report "matches".
        let output = gather_diff_with("pf", ArtifactKind::Skill, &ctx).unwrap();

        assert!(!output.is_up_to_date, "must not claim up-to-date while a copy differs");
        assert_eq!(output.copies.len(), 2, "both platform copies surveyed");
        let focus = output.copies.iter().find(|c| c.is_focus).expect("a focus copy");
        assert!(focus.platforms.contains(&crate::platform::Platform::Codex), "focuses Codex");
        assert!(!focus.matches, "the focused copy differs");
        assert!(
            output
                .copies
                .iter()
                .any(|c| c.platforms.contains(&crate::platform::Platform::Claude) && c.matches),
            "the Claude copy is reported as matching"
        );
        assert!(
            output.reconciliations[0].command.contains("--from codex"),
            "reconcile qualified to the diverging platform: {:?}",
            output.reconciliations[0]
        );
    }

    // --- diff_with_analysis (llm-feature only) ---

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn full_mode_never_invokes_llm() {
        use crate::gateway::fakes::FakeLlmClient;

        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            "different installed content",
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let llm = FakeLlmClient::new("x");
        let ctx = t.ctx().with_llm(&llm);
        let output = diff_with_analysis("my-agent", ArtifactKind::Agent, true, &ctx)
            .await
            .expect("full diff must succeed");

        assert!(llm.all_calls().is_empty(), "--full must never touch the LLM");
        assert!(output.analysis.is_none());
        assert!(output.analysis_note.is_none());
    }

    #[cfg(feature = "llm")]
    #[tokio::test]
    async fn failing_llm_degrades_to_note_not_error() {
        use crate::gateway::fakes::FakeLlmClient;

        let t = TestContext::new();
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            "different installed content",
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            InstallScope::Global,
        );

        let mut llm = FakeLlmClient::new("x");
        llm.should_fail = true;
        let ctx = t.ctx().with_llm(&llm);
        let output = diff_with_analysis("my-agent", ArtifactKind::Agent, false, &ctx)
            .await
            .expect("a failing LLM must not fail the command");

        assert!(output.analysis.is_none());
        let note = output.analysis_note.expect("a note must be set when the LLM fails");
        assert!(note.contains("LLM summary unavailable"), "{note}");
        assert!(note.contains("--full"), "{note}");
        assert!(note.contains("configured to fail"), "condensed reason present: {note}");
        assert!(!note.contains('\n'), "no raw multi-line error body: {note}");
    }
}
