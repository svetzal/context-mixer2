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

pub async fn diff_with(name: &str, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<()> {
    source::auto_update_all_with(ctx)?;

    // Find the installed file on disk (global then local)
    let (installed_path, local) = find_installed_on_disk_with(name, kind, ctx)?;

    // Find the source artifact by scanning all sources
    let (source_path, source_name, source_version) = find_in_sources_with(name, kind, ctx)?;

    // Compare checksums
    let installed_checksum = checksum::checksum_artifact_with(&installed_path, kind, ctx.fs)?;
    let source_checksum = checksum::checksum_artifact_with(&source_path, kind, ctx.fs)?;

    if installed_checksum == source_checksum {
        println!("{name} is up to date with source.");
        return Ok(());
    }

    // Get installed version from lock file if available
    let lock = lockfile::load_with(local, ctx.fs, ctx.paths)?;
    let installed_version = lock
        .packages
        .get(name)
        .and_then(|e| e.version.as_deref())
        .unwrap_or("unversioned");

    let source_ver_display = source_version.as_deref().unwrap_or("unversioned");
    let scope = if local { "local" } else { "global" };

    println!("Comparing {name} ({kind})");
    println!("  Installed ({scope}): {installed_version}");
    println!("  Source ({source_name}): {source_ver_display}");
    println!();

    // Build diff text
    let diff_text = match kind {
        ArtifactKind::Agent => diff_files_with(&installed_path, &source_path, ctx)?,
        ArtifactKind::Skill => diff_dirs_with(&installed_path, &source_path, ctx)?,
    };

    println!("Analyzing differences...");
    println!();

    let system_prompt = "You are a technical analyst comparing two versions of an AI coding assistant artifact (an agent definition or skill definition written in markdown). \
        Provide a clear, concise summary of the differences. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed\n\
        2. Whether the update is significant or cosmetic\n\
        3. A recommendation: should the user update their installed version?\n\n\
        Keep your analysis brief and actionable — a few paragraphs at most.";

    let user_prompt = format!(
        "Compare these two versions of the {kind} '{name}':\n\
        - Installed version: {installed_version}\n\
        - Source version: {source_ver_display}\n\n\
        {diff_text}"
    );

    let analysis = match ctx.llm {
        Some(llm) => llm.analyze(system_prompt, &user_prompt).await?,
        None => bail!("LLM client not configured for diff analysis"),
    };
    println!("{analysis}");

    Ok(())
}

fn find_installed_on_disk_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, bool)> {
    for local in [false, true] {
        let dir = ctx.paths.install_dir(kind, local);
        let path = kind.installed_path(name, &dir);
        if ctx.fs.exists(&path) {
            return Ok((path, local));
        }
    }

    bail!("No installed {kind} named '{name}' found on disk.");
}

fn find_in_sources_with(
    name: &str,
    kind: ArtifactKind,
    ctx: &AppContext<'_>,
) -> Result<(PathBuf, String, Option<String>)> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        if sa.artifact.name == name && sa.artifact.kind == kind {
            return Ok((sa.artifact.path, sa.source_name, sa.artifact.version));
        }
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
    use crate::lockfile;
    use crate::test_support::{
        agent_content, install_agent_on_disk, install_skill_on_disk, make_ctx,
        setup_source_with_agent, test_paths,
    };
    use crate::types::{ArtifactKind, LockEntry, LockFile, LockSource};
    use chrono::Utc;
    use std::collections::BTreeMap;

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

    // --- find_installed_on_disk_with ---

    #[test]
    fn find_installed_finds_global_agent() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_agent_on_disk(&fs, &paths, "my-agent", "content", false);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_installed_on_disk_with("my-agent", ArtifactKind::Agent, &ctx);

        assert!(result.is_ok(), "expected Ok: {:?}", result.err());
        let (_, local) = result.unwrap();
        assert!(!local, "expected global (local=false)");
    }

    #[test]
    fn find_installed_finds_local_agent() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_agent_on_disk(&fs, &paths, "my-agent", "content", true);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_installed_on_disk_with("my-agent", ArtifactKind::Agent, &ctx);

        assert!(result.is_ok(), "expected Ok: {:?}", result.err());
        let (_, local) = result.unwrap();
        assert!(local, "expected local (local=true)");
    }

    #[test]
    fn find_installed_prefers_global_over_local() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_agent_on_disk(&fs, &paths, "my-agent", "global content", false);
        install_agent_on_disk(&fs, &paths, "my-agent", "local content", true);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let (_, local) =
            find_installed_on_disk_with("my-agent", ArtifactKind::Agent, &ctx).unwrap();

        assert!(!local, "expected global to be preferred over local");
    }

    #[test]
    fn find_installed_errors_when_not_found() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_installed_on_disk_with("nonexistent", ArtifactKind::Agent, &ctx);

        assert!(result.is_err());
    }

    #[test]
    fn find_installed_finds_skill_directory() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        install_skill_on_disk(&fs, &paths, "my-skill", &[("SKILL.md", "skill content")], false);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = find_installed_on_disk_with("my-skill", ArtifactKind::Skill, &ctx);

        assert!(result.is_ok(), "expected Ok: {:?}", result.err());
        let (_, local) = result.unwrap();
        assert!(!local, "expected global (local=false)");
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
        fs.add_file("/sources/my-source/my-agent.md", content.clone());
        install_agent_on_disk(&fs, &paths, "my-agent", &content, false);

        // Write a lock file entry so load_with succeeds
        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: Utc::now().to_rfc3339(),
                source: LockSource {
                    repo: "my-source".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:placeholder".to_string(),
                installed_checksum: "sha256:placeholder".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

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

        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: Utc::now().to_rfc3339(),
                source: LockSource {
                    repo: "my-source".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:placeholder".to_string(),
                installed_checksum: "sha256:placeholder".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

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

        let mut lock = LockFile {
            version: 1,
            packages: BTreeMap::new(),
        };
        lock.packages.insert(
            "my-agent".to_string(),
            LockEntry {
                artifact_type: ArtifactKind::Agent,
                version: Some("1.0.0".to_string()),
                installed_at: Utc::now().to_rfc3339(),
                source: LockSource {
                    repo: "my-source".to_string(),
                    path: "my-agent.md".to_string(),
                },
                source_checksum: "sha256:placeholder".to_string(),
                installed_checksum: "sha256:placeholder".to_string(),
            },
        );
        lockfile::save_with(&lock, false, &fs, &paths).unwrap();

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
}
