//! LLM-backed prose summary for `cmx info` (feature-gated).

use crate::error::{CliError, Result};

use crate::context::AppContext;
use crate::types::ArtifactKind;

use super::ArtifactInfo;

/// Generate a short prose paragraph describing what the artifact does, using the
/// configured LLM provider. Backs the "What it does" line of `cmx skill info`.
///
/// Read-only and best-effort: callers treat a failure as "no summary" rather
/// than a hard error, so `info` still prints everything else. Requires
/// `ctx.llm` to be set (an `llm`-feature build with a configured gateway).
pub async fn summarize(info: &ArtifactInfo, ctx: &AppContext<'_>) -> Result<String> {
    let Some(llm) = ctx.llm else {
        return Err(CliError::LlmNotConfigured);
    };

    let content = read_summary_source(info, ctx)?;
    // Cap the context we send — the frontmatter and opening prose carry the
    // intent; the long tail rarely changes a one-paragraph summary.
    let excerpt: String = content.chars().take(6000).collect();

    let system_prompt = "You are summarizing an AI coding assistant artifact (an agent or skill defined in markdown) \
        for someone browsing their installed tools. Write ONE plain-prose paragraph of 2-4 sentences describing what it does \
        and the kind of task it helps with. Be concrete and neutral. Do not use headings, lists, or markdown formatting, \
        and do not begin by restating the artifact's name as a definition.";
    let user_prompt = format!("Summarize the {} named '{}':\n\n{excerpt}", info.kind, info.name);

    let summary = llm.analyze(system_prompt, &user_prompt).await?.trim().to_string();
    if summary.is_empty() {
        return Err(CliError::LlmEmptySummary);
    }
    Ok(summary)
}

/// The text to feed the summarizer. Prefers the artifact's primary content file
/// (`SKILL.md` / the agent `.md`), but falls back for skill *bundles* that lack a
/// top-level `SKILL.md` — `DESCRIPTION.md`, then any top-level markdown — so a
/// multi-skill directory (e.g. Hermes' `productivity`) still summarizes.
fn read_summary_source(info: &ArtifactInfo, ctx: &AppContext<'_>) -> Result<String> {
    if let Ok(content) = ctx.fs.read_to_string(&info.kind.content_path(&info.path)) {
        return Ok(content);
    }
    if info.kind == ArtifactKind::Skill && ctx.fs.is_dir(&info.path) {
        for candidate in ["DESCRIPTION.md", "README.md"] {
            if let Ok(content) = ctx.fs.read_to_string(&info.path.join(candidate)) {
                return Ok(content);
            }
        }
        // Last resort: concatenate the directory's top-level markdown files.
        let mut entries = ctx.fs.read_dir(&info.path)?;
        entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        let mut combined = String::new();
        for entry in entries {
            if !entry.is_dir && entry.file_name.to_ascii_lowercase().ends_with(".md") {
                if let Ok(text) = ctx.fs.read_to_string(&entry.path) {
                    combined.push_str(&text);
                    combined.push('\n');
                }
            }
        }
        if !combined.trim().is_empty() {
            return Ok(combined);
        }
    }
    Err(CliError::NoReadableContentToSummarize {
        path: info.path.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::AppContext;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient, FakeLlmClient};
    use crate::test_support::test_paths;
    use crate::types::{ArtifactKind, InstallScope};
    use chrono::Utc;
    use std::path::PathBuf;

    fn minimal_info(name: &str, kind: ArtifactKind) -> ArtifactInfo {
        ArtifactInfo {
            name: name.to_string(),
            kind,
            scope: "global",
            path: PathBuf::from(format!("{name}.md")),
            version: None,
            installed_at: None,
            source_display: None,
            source_checksum: None,
            installed_checksum: None,
            disk_checksum: None,
            locally_modified: false,
            untracked: false,
            deprecation: None,
            available_version: None,
            skill_files: vec![],
            activates_when: None,
            summary: None,
            summary_error: None,
        }
    }

    #[tokio::test]
    async fn summarize_returns_llm_paragraph() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("It packages curated context.");

        let dir = paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("my-skill");
        fs.add_file(dir.join("SKILL.md"), crate::test_support::skill_content("Use when X"));

        let mut info = minimal_info("my-skill", ArtifactKind::Skill);
        info.path = dir;

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        let summary = summarize(&info, &ctx).await.unwrap();
        assert_eq!(summary, "It packages curated context.");
    }

    #[tokio::test]
    async fn summarize_errors_without_llm() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let info = minimal_info("x", ArtifactKind::Skill);
        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: None,
        };
        let err = summarize(&info, &ctx).await.unwrap_err().to_string();
        assert!(err.contains("LLM"), "expected llm-not-configured error: {err}");
    }

    #[tokio::test]
    async fn summarize_falls_back_to_description_for_bundle_without_skill_md() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("A bundle of productivity tools.");

        // A skill *bundle*: no top-level SKILL.md, just DESCRIPTION.md + sub-skills.
        let dir = paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("productivity");
        fs.add_file(dir.join("DESCRIPTION.md"), "# Productivity\nAirtable, Notion, and more.");
        fs.add_file(dir.join("notion").join("SKILL.md"), crate::test_support::skill_content("n"));

        let mut info = minimal_info("productivity", ArtifactKind::Skill);
        info.path = dir;
        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        // Falls back to DESCRIPTION.md instead of failing on the missing SKILL.md.
        assert_eq!(summarize(&info, &ctx).await.unwrap(), "A bundle of productivity tools.");
    }

    #[tokio::test]
    async fn summarize_errors_clearly_when_no_readable_content() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("unused");

        let dir = paths
            .install_dir(ArtifactKind::Skill, InstallScope::Global)
            .unwrap()
            .join("empty");
        fs.add_dir(&dir); // exists but has no markdown content

        let mut info = minimal_info("empty", ArtifactKind::Skill);
        info.path = dir;
        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        let err = summarize(&info, &ctx).await.unwrap_err().to_string();
        assert!(err.contains("no readable content"), "clear content error: {err}");
    }
}
