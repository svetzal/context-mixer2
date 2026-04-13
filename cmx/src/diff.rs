use anyhow::{Context, Result, bail};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::source;
use crate::source_iter;
use crate::types::ArtifactKind;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DiffOutput {
    pub artifact_name: String,
    pub kind: ArtifactKind,
    pub is_up_to_date: bool,
    pub installed_version: Option<String>,
    pub source_version: Option<String>,
    pub source_name: String,
    pub diff_text: Option<String>,
    pub analysis: Option<String>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn diff_with(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<DiffOutput> {
    gather_diff_with(name, kind, ctx).await
}

// ---------------------------------------------------------------------------
// Gather (no println!)
// ---------------------------------------------------------------------------

pub(crate) async fn gather_diff_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<DiffOutput> {
    source::auto_update_all_with(ctx)?;

    // Find the installed file on disk (global then local)
    let (installed_path, local) = config::find_installed_path(name, kind, ctx.fs, ctx.paths)
        .with_context(|| format!("No installed {kind} named '{name}' found on disk."))?;

    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources_with(name, kind, ctx)?;

    // Compare checksums
    let installed_checksum = checksum::checksum_artifact_with(&installed_path, kind, ctx.fs)?;
    let source_checksum = checksum::checksum_artifact_with(&source_path, kind, ctx.fs)?;

    if installed_checksum == source_checksum {
        return Ok(DiffOutput {
            artifact_name: name.to_string(),
            kind,
            is_up_to_date: true,
            installed_version: None,
            source_version,
            source_name,
            diff_text: None,
            analysis: None,
        });
    }

    // Get installed version from lock file if available
    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let installed_version = lock.packages.get(name).and_then(|e| e.version.clone());

    // Build diff text
    let diff_text = match kind {
        ArtifactKind::Agent => diff_files_with(&installed_path, &source_path, ctx)?,
        ArtifactKind::Skill => diff_dirs_with(&installed_path, &source_path, ctx)?,
    };

    let installed_ver_display = installed_version.as_deref().unwrap_or("unversioned");
    let source_ver_display = source_version.as_deref().unwrap_or("unversioned");

    let system_prompt = "You are a technical analyst comparing two versions of an AI coding assistant artifact (an agent definition or skill definition written in markdown). \
        Provide a clear, concise summary of the differences. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed\n\
        2. Whether the update is significant or cosmetic\n\
        3. A recommendation: should the user update their installed version?\n\n\
        Keep your analysis brief and actionable — a few paragraphs at most.";

    let user_prompt = format!(
        "Compare these two versions of the {kind} '{name}':\n\
        - Installed version: {installed_ver_display}\n\
        - Source version: {source_ver_display}\n\n\
        {diff_text}"
    );

    let analysis = match ctx.llm {
        Some(llm) => llm.analyze(system_prompt, &user_prompt).await?,
        None => bail!("LLM client not configured for diff analysis"),
    };

    Ok(DiffOutput {
        artifact_name: name.to_string(),
        kind,
        is_up_to_date: false,
        installed_version,
        source_version,
        source_name,
        diff_text: Some(diff_text),
        analysis: Some(analysis),
    })
}

// ---------------------------------------------------------------------------
// Print (no business logic)
// ---------------------------------------------------------------------------

pub fn print_diff_output(output: &DiffOutput) {
    if output.is_up_to_date {
        println!("{} is up to date with source.", output.artifact_name);
        return;
    }

    let installed_ver = output.installed_version.as_deref().unwrap_or("unversioned");
    let source_ver = output.source_version.as_deref().unwrap_or("unversioned");

    println!("Comparing {} ({})", output.artifact_name, output.kind);
    println!("  Installed: {installed_ver}");
    println!("  Source ({}): {source_ver}", output.source_name);
    println!();

    if let Some(analysis) = &output.analysis {
        println!("Analyzing differences...");
        println!();
        println!("{analysis}");
    } else if let Some(diff) = &output.diff_text {
        // No LLM analysis available — show raw diff
        println!("Differences:");
        println!("{diff}");
    }
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

fn diff_files_with(installed: &Path, source: &Path, ctx: &AppContext<'_>) -> Result<String> {
    let installed_content = ctx
        .fs
        .read_to_string(installed)
        .with_context(|| format!("Failed to read {}", installed.display()))?;
    let source_content = ctx
        .fs
        .read_to_string(source)
        .with_context(|| format!("Failed to read {}", source.display()))?;

    Ok(format!(
        "=== INSTALLED VERSION ===\n{installed_content}\n\n=== SOURCE VERSION ===\n{source_content}"
    ))
}

fn diff_dirs_with(installed: &Path, source: &Path, ctx: &AppContext<'_>) -> Result<String> {
    let mut result = String::new();

    let installed_files = collect_relative_files_with(installed, ctx)?;
    let source_files = collect_relative_files_with(source, ctx)?;

    for f in &installed_files {
        if !source_files.contains(f) {
            let _ = writeln!(result, "--- Only in installed: {f}");
        }
    }

    for f in &source_files {
        if !installed_files.contains(f) {
            let _ = writeln!(result, "+++ Only in source: {f}");
        }
    }

    for f in &installed_files {
        if source_files.contains(f) {
            let i_path = installed.join(f);
            let s_path = source.join(f);
            let i_content = ctx.fs.read_to_string(&i_path).unwrap_or_default();
            let s_content = ctx.fs.read_to_string(&s_path).unwrap_or_default();
            if i_content != s_content {
                let _ = write!(
                    result,
                    "\n=== {f} (INSTALLED) ===\n{i_content}\n=== {f} (SOURCE) ===\n{s_content}\n"
                );
            }
        }
    }

    Ok(result)
}

fn collect_relative_files_with(dir: &Path, ctx: &AppContext<'_>) -> Result<Vec<String>> {
    let mut files = collect_files_with(dir, ctx)?
        .into_iter()
        .map(|p| p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().to_string())
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn collect_files_with(dir: &Path, ctx: &AppContext<'_>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = ctx.fs.read_dir(dir)?;
    for entry in entries {
        if entry.file_name.starts_with('.') {
            continue;
        }
        if entry.is_dir {
            files.extend(collect_files_with(&entry.path, ctx)?);
        } else {
            files.push(entry.path);
        }
    }
    Ok(files)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient, FakeLlmClient};
    use crate::test_support::{
        agent_content, install_agent_on_disk, make_ctx, make_lock_entry_versioned,
        save_lock_with_entry, setup_source_with_agent, test_paths,
    };
    use crate::types::ArtifactKind;
    use chrono::Utc;

    // --- collect_relative_files_with ---

    #[test]
    fn collect_relative_files_returns_sorted_relative_paths() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/dir/b.md", "b");
        fs.add_file("/dir/a.md", "a");
        fs.add_file("/dir/sub/c.md", "c");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = collect_relative_files_with(std::path::Path::new("/dir"), &ctx).unwrap();

        assert_eq!(result, vec!["a.md", "b.md", "sub/c.md"]);
    }

    #[test]
    fn collect_relative_files_empty_dir_returns_empty() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_dir("/empty");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = collect_relative_files_with(std::path::Path::new("/empty"), &ctx).unwrap();

        assert!(result.is_empty());
    }

    // --- diff_files_with ---

    #[test]
    fn diff_files_builds_comparison_text() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/installed/agent.md", "installed content");
        fs.add_file("/source/agent.md", "source content");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_files_with(
            std::path::Path::new("/installed/agent.md"),
            std::path::Path::new("/source/agent.md"),
            &ctx,
        )
        .unwrap();

        assert!(
            result.contains("=== INSTALLED VERSION ==="),
            "missing installed header: {result}"
        );
        assert!(result.contains("=== SOURCE VERSION ==="), "missing source header: {result}");
        assert!(result.contains("installed content"), "missing installed content: {result}");
        assert!(result.contains("source content"), "missing source content: {result}");
    }

    #[test]
    fn diff_files_errors_on_missing_installed() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/source/agent.md", "source content");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_files_with(
            std::path::Path::new("/installed/agent.md"),
            std::path::Path::new("/source/agent.md"),
            &ctx,
        );

        assert!(result.is_err());
    }

    // --- diff_dirs_with ---

    #[test]
    fn diff_dirs_identical_directories_returns_empty() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = "---\ndescription: My skill\n---\n";
        fs.add_file("/installed/my-skill/SKILL.md", content);
        fs.add_file("/source/my-skill/SKILL.md", content);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_dirs_with(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            &ctx,
        )
        .unwrap();

        assert!(result.is_empty(), "expected empty diff for identical dirs, got: {result}");
    }

    #[test]
    fn diff_dirs_shows_files_only_in_installed() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/installed/my-skill/SKILL.md", "skill");
        fs.add_file("/installed/my-skill/extra.md", "extra");
        fs.add_file("/source/my-skill/SKILL.md", "skill");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_dirs_with(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            &ctx,
        )
        .unwrap();

        assert!(result.contains("Only in installed"), "expected 'Only in installed': {result}");
        assert!(result.contains("extra.md"), "expected extra.md: {result}");
    }

    #[test]
    fn diff_dirs_shows_files_only_in_source() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/installed/my-skill/SKILL.md", "skill");
        fs.add_file("/source/my-skill/SKILL.md", "skill");
        fs.add_file("/source/my-skill/new-file.md", "new");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_dirs_with(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            &ctx,
        )
        .unwrap();

        assert!(result.contains("Only in source"), "expected 'Only in source': {result}");
        assert!(result.contains("new-file.md"), "expected new-file.md: {result}");
    }

    #[test]
    fn diff_dirs_shows_changed_file_content() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        fs.add_file("/installed/my-skill/SKILL.md", "installed skill content");
        fs.add_file("/source/my-skill/SKILL.md", "updated skill content");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_dirs_with(
            std::path::Path::new("/installed/my-skill"),
            std::path::Path::new("/source/my-skill"),
            &ctx,
        )
        .unwrap();

        assert!(result.contains("INSTALLED"), "expected INSTALLED section: {result}");
        assert!(result.contains("SOURCE"), "expected SOURCE section: {result}");
        assert!(
            result.contains("installed skill content"),
            "missing installed content: {result}"
        );
        assert!(result.contains("updated skill content"), "missing source content: {result}");
    }

    // --- find_in_sources_with ---

    #[test]
    fn find_in_sources_locates_agent() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_in_sources_with("my-agent", ArtifactKind::Agent, &ctx);

        assert!(result.is_ok(), "expected Ok: {:?}", result.err());
    }

    #[test]
    fn find_in_sources_errors_when_not_found() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "other-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_in_sources_with("my-agent", ArtifactKind::Agent, &ctx);

        assert!(result.is_err());
    }

    #[test]
    fn find_in_sources_matches_kind() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Source has an agent named "my-agent"; searching for a skill with same name should fail
        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_in_sources_with("my-agent", ArtifactKind::Skill, &ctx);

        assert!(result.is_err(), "expected Err when kind doesn't match");
    }

    // --- diff_with (top-level async) ---

    #[tokio::test]
    async fn diff_with_reports_up_to_date_when_checksums_match() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        // Override the source file with specific content
        fs.add_file("/sources/my-source/agents/my-agent.md", content.clone());
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        // Write a lock file entry so load_with succeeds
        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            false,
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_with("my-agent", ArtifactKind::Agent, &ctx).await;

        // Same content => checksums match => returns Ok immediately (no LLM needed)
        assert!(result.is_ok(), "expected Ok for up-to-date artifact: {:?}", result.err());
    }

    #[tokio::test]
    async fn diff_with_errors_without_llm_when_checksums_differ() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        // Install a different version so checksums differ
        install_agent_on_disk(&fs, &paths, "my-agent", "different installed content", false);

        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            false,
        );

        // No LLM configured — should bail when it tries to analyze
        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = diff_with("my-agent", ArtifactKind::Agent, &ctx).await;

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("LLM"), "expected LLM error, got: {msg}");
    }

    #[tokio::test]
    async fn diff_with_succeeds_with_llm_when_checksums_differ() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("LLM analysis result");

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(&fs, &paths, "my-agent", "different installed content", false);

        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            false,
        );

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        let result = diff_with("my-agent", ArtifactKind::Agent, &ctx).await;

        assert!(result.is_ok(), "expected Ok with LLM configured: {:?}", result.err());
    }

    // --- gather_diff_with: assert on DiffOutput struct fields ---

    #[tokio::test]
    async fn gather_diff_sets_is_up_to_date_when_checksums_match() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let content = agent_content("my-agent", "A test agent");
        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        fs.add_file("/sources/my-source/agents/my-agent.md", content.clone());
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            false,
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let output = gather_diff_with("my-agent", ArtifactKind::Agent, &ctx).await.unwrap();

        assert!(output.is_up_to_date, "expected is_up_to_date = true");
        assert_eq!(output.artifact_name, "my-agent");
        assert_eq!(output.kind, ArtifactKind::Agent);
        assert_eq!(output.source_name, "my-source");
        assert!(output.analysis.is_none(), "no analysis for up-to-date artifact");
        assert!(output.diff_text.is_none(), "no diff_text for up-to-date artifact");
    }

    #[tokio::test]
    async fn gather_diff_sets_analysis_when_checksums_differ_and_llm_present() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("LLM analysis result");

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        install_agent_on_disk(&fs, &paths, "my-agent", "different installed content", false);

        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            false,
        );

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        let output = gather_diff_with("my-agent", ArtifactKind::Agent, &ctx).await.unwrap();

        assert!(!output.is_up_to_date, "expected is_up_to_date = false");
        assert!(output.analysis.is_some(), "expected analysis to be present");
        assert_eq!(output.analysis.as_deref(), Some("LLM analysis result"));
        assert!(output.diff_text.is_some(), "expected diff_text to be present");
        assert_eq!(output.installed_version.as_deref(), Some("1.0.0"));
    }

    // --- failure-path tests ---

    #[tokio::test]
    async fn diff_returns_error_when_llm_call_fails() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient {
            response: String::new(),
            should_fail: true,
        };

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");
        // Install a different version so checksums differ and LLM is invoked
        install_agent_on_disk(&fs, &paths, "my-agent", "different installed content", false);

        save_lock_with_entry(
            &fs,
            &paths,
            "my-agent",
            make_lock_entry_versioned(ArtifactKind::Agent, "1.0.0", "my-source", "my-agent.md"),
            false,
        );

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        let result = diff_with("my-agent", ArtifactKind::Agent, &ctx).await;

        assert!(result.is_err(), "expected Err when LLM call fails");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("configured to fail"), "unexpected error message: {msg}");
    }
}
