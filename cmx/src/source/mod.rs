use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config;
use crate::context::AppContext;
use crate::gateway::Filesystem;
use crate::scan;
use crate::source_iter;
use crate::source_update;
use crate::types::{ArtifactKind, SourceEntry, SourceType};

mod browse;
pub use browse::{BrowseArtifact, BrowseSkill, SourceBrowseResult};
pub(crate) use browse::{build_browse_result, count_artifacts, dir_entry_names};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub use crate::scan::ScanWarning;

#[derive(Clone, Debug)]
pub struct SourceScanResult {
    pub name: String,
    pub agents_found: usize,
    pub skills_found: usize,
    pub warnings: Vec<ScanWarning>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceListEntry {
    pub name: String,
    /// In JSON output this appears as `"type"` (matching the `--json` contract).
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub location: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceListResult {
    /// In JSON output this appears as `"sources"`.
    #[serde(rename = "sources")]
    pub entries: Vec<SourceListEntry>,
}

#[derive(Clone, Debug)]
pub struct SourceRemoveResult {
    pub name: String,
    pub clone_deleted: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn add(name: &str, path_or_url: &str, ctx: &AppContext<'_>) -> Result<SourceScanResult> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;

    if sources.sources.contains_key(name) {
        bail!("Source '{name}' already exists. Remove it first to re-register.");
    }

    let entry = if looks_like_url(path_or_url) {
        add_git_source_with(name, path_or_url, ctx)?
    } else {
        add_local_source_with(path_or_url, ctx)?
    };

    let (agents_found, skills_found, warnings) = scan_and_count(&entry, ctx.fs)?;

    config::mutate_sources(ctx.fs, ctx.paths, |sources| {
        sources.sources.insert(name.to_string(), entry);
        Ok(())
    })?;

    Ok(SourceScanResult {
        name: name.to_string(),
        agents_found,
        skills_found,
        warnings,
    })
}

pub fn list(ctx: &AppContext<'_>) -> Result<SourceListResult> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;

    let entries = sources
        .sources
        .iter()
        .map(|(name, entry)| {
            let location = match entry.source_type {
                SourceType::Local => entry.path.as_ref().map(|p| p.display().to_string()),
                SourceType::Git => entry.url.clone(),
            };
            let kind = match entry.source_type {
                SourceType::Local => "local",
                SourceType::Git => "git",
            };
            SourceListEntry {
                name: name.clone(),
                kind,
                location: location.unwrap_or_else(|| "<no location>".to_string()),
            }
        })
        .collect();

    Ok(SourceListResult { entries })
}

pub fn browse(name: &str, ctx: &AppContext<'_>) -> Result<SourceBrowseResult> {
    source_update::auto_update_source(name, ctx)?;

    let sources = config::load_sources(ctx.fs, ctx.paths)?;

    let entry = sources
        .get_source(name)
        .context("Run 'cmx source list' to see registered sources.")?;

    let local_path = config::resolve_local_path(entry)?;
    if !ctx.fs.exists(&local_path) {
        bail!(
            "Source path {} does not exist. {}",
            local_path.display(),
            match entry.source_type {
                SourceType::Git => "Try 'cmx source update' to fetch it.",
                SourceType::Local => "Check that the directory still exists.",
            }
        );
    }

    let all_artifacts = source_iter::each_source_artifact(&sources.sources, ctx.fs)?;
    let artifacts: Vec<_> = all_artifacts
        .into_iter()
        .filter(|sa| sa.source_name == name)
        .map(|sa| sa.artifact)
        .collect();

    // Imperative shell: pre-load skill directory listings keyed by artifact path
    let skill_dirs: HashMap<PathBuf, Vec<String>> = artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Skill)
        .map(|s| {
            let files = ctx
                .fs
                .read_dir(&s.path)
                .map(|entries| dir_entry_names(&entries))
                .unwrap_or_default();
            (s.path.clone(), files)
        })
        .collect();

    Ok(build_browse_result(name, &artifacts, &skill_dirs))
}

pub fn remove(name: &str, ctx: &AppContext<'_>) -> Result<SourceRemoveResult> {
    let entry = config::mutate_sources(ctx.fs, ctx.paths, |sources| {
        sources
            .sources
            .remove(name)
            .with_context(|| format!("Source '{name}' not found."))
    })?;

    let clone_deleted = if let Some(clone_path) = &entry.local_clone {
        if ctx.fs.exists(clone_path) {
            ctx.fs.remove_dir_all(clone_path).with_context(|| {
                format!("Failed to remove cloned repo at {}", clone_path.display())
            })?;
            true
        } else {
            false
        }
    } else {
        false
    };

    Ok(SourceRemoveResult {
        name: name.to_string(),
        clone_deleted,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn add_local_source_with(path_str: &str, ctx: &AppContext<'_>) -> Result<SourceEntry> {
    let path = PathBuf::from(path_str);
    let path = ctx
        .fs
        .canonicalize(&path)
        .with_context(|| format!("Path '{path_str}' does not exist or is not accessible."))?;

    if !ctx.fs.is_dir(&path) {
        bail!("'{}' is not a directory.", path.display());
    }

    Ok(SourceEntry {
        source_type: SourceType::Local,
        path: Some(path),
        url: None,
        local_clone: None,
        branch: None,
        last_updated: Some(ctx.clock.now().to_rfc3339()),
    })
}

fn add_git_source_with(name: &str, url: &str, ctx: &AppContext<'_>) -> Result<SourceEntry> {
    let clone_dir = ctx.paths.git_clones_dir().join(name);

    if ctx.fs.exists(&clone_dir) {
        bail!(
            "Clone directory {} already exists. Remove it or choose a different name.",
            clone_dir.display()
        );
    }

    ctx.git.clone_repo(url, &clone_dir)?;

    Ok(SourceEntry {
        source_type: SourceType::Git,
        path: None,
        url: Some(url.to_string()),
        local_clone: Some(clone_dir),
        branch: Some("main".to_string()),
        last_updated: Some(ctx.clock.now().to_rfc3339()),
    })
}

pub(crate) fn scan_and_count(
    entry: &crate::types::SourceEntry,
    fs: &dyn Filesystem,
) -> Result<(usize, usize, Vec<ScanWarning>)> {
    let local_path = config::resolve_local_path(entry)?;
    let scan_result = scan::scan_source(&local_path, fs)?;
    let (agents_found, skills_found) = count_artifacts(&scan_result.artifacts);
    Ok((agents_found, skills_found, scan_result.warnings))
}

pub fn looks_like_url(s: &str) -> bool {
    s.starts_with("https://")
        || s.starts_with("http://")
        || s.starts_with("git@")
        || s.starts_with("ssh://")
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{
        TestContext, make_ctx, make_git_entry, make_local_entry, setup_empty_sources,
        setup_sources_from_entries, test_paths,
    };
    use chrono::Utc;
    use std::cell::RefCell;
    use std::path::PathBuf;

    // --- looks_like_url ---

    #[test]
    fn looks_like_url_https() {
        assert!(looks_like_url("https://github.com/foo/bar"));
    }

    #[test]
    fn looks_like_url_http() {
        assert!(looks_like_url("http://example.com"));
    }

    #[test]
    fn looks_like_url_git_at() {
        assert!(looks_like_url("git@github.com:foo/bar.git"));
    }

    #[test]
    fn looks_like_url_ssh() {
        assert!(looks_like_url("ssh://git@example.com/repo.git"));
    }

    #[test]
    fn looks_like_url_absolute_path() {
        assert!(!looks_like_url("/home/user/repos/guidelines"));
    }

    #[test]
    fn looks_like_url_relative_path() {
        assert!(!looks_like_url("./relative/path"));
    }

    #[test]
    fn looks_like_url_plain_name() {
        assert!(!looks_like_url("just-a-name"));
    }

    // --- source management business logic tests ---

    #[test]
    fn add_bails_when_source_name_already_exists() {
        let t = TestContext::new();

        // Pre-populate with existing source
        setup_sources_from_entries(
            &t.fs,
            &t.paths,
            &[("my-source", make_local_entry("/existing", None))],
        );

        let ctx = t.ctx();
        let result = add("my-source", "/new/path", &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("already exists"), "unexpected: {msg}");
    }

    #[test]
    fn add_detects_local_path_no_git_call() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        // Set up a valid local directory
        t.fs.add_dir("/local/repo");

        let ctx = t.ctx();
        let result = add("local-source", "/local/repo", &ctx);
        assert!(result.is_ok(), "expected ok: {:?}", result.err());

        // No git clone should have been called
        assert!(t.git.cloned.borrow().is_empty(), "no git clone expected for local path");
    }

    #[test]
    fn add_result_has_correct_name_and_counts() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);
        t.fs.add_dir("/local/repo");

        let ctx = t.ctx();
        let result = add("local-source", "/local/repo", &ctx).unwrap();

        assert_eq!(result.name, "local-source");
        assert_eq!(result.agents_found, 0, "empty repo has no agents");
        assert_eq!(result.skills_found, 0, "empty repo has no skills");
    }

    #[test]
    fn add_detects_url_and_clones() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = add("git-source", "https://github.com/example/repo.git", &ctx);
        assert!(result.is_ok(), "expected ok: {:?}", result.err());

        let cloned = t.git.cloned.borrow();
        assert_eq!(cloned.len(), 1, "expected one git clone");
        assert_eq!(cloned[0].0, "https://github.com/example/repo.git");
    }

    #[test]
    fn add_saves_sources_after_registration() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);
        t.fs.add_dir("/local/repo");

        let ctx = t.ctx();
        add("new-source", "/local/repo", &ctx).unwrap();

        let sources = config::load_sources(&t.fs, &t.paths).unwrap();
        assert!(sources.sources.contains_key("new-source"), "source should be saved");
    }

    #[test]
    fn gather_list_empty_sources_returns_empty_entries() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = list(&ctx).unwrap();

        assert!(result.entries.is_empty(), "expected empty entries for no sources");
    }

    #[test]
    fn gather_list_local_source_has_correct_kind_and_location() {
        let t = TestContext::new();

        setup_sources_from_entries(
            &t.fs,
            &t.paths,
            &[("my-source", make_local_entry("/local/repo", None))],
        );

        let ctx = t.ctx();
        let result = list(&ctx).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "my-source");
        assert_eq!(result.entries[0].kind, "local");
        assert_eq!(result.entries[0].location, "/local/repo");
    }

    #[test]
    fn remove_result_reports_clone_deleted() {
        let t = TestContext::new();

        let clone_path = PathBuf::from("/clones/git-source");
        setup_sources_from_entries(
            &t.fs,
            &t.paths,
            &[(
                "git-source",
                make_git_entry(
                    "https://github.com/example/repo.git",
                    clone_path.clone(),
                    "main",
                    None,
                ),
            )],
        );
        t.fs.add_file(clone_path.join("README.md"), "# repo");

        let ctx = t.ctx();
        let result = remove("git-source", &ctx).unwrap();

        assert_eq!(result.name, "git-source");
        assert!(result.clone_deleted, "expected clone_deleted to be true");
        assert!(!t.fs.exists(&clone_path), "clone directory should be removed");
    }

    #[test]
    fn remove_deletes_clone_directory_for_git_source() {
        let t = TestContext::new();

        let clone_path = PathBuf::from("/clones/git-source");
        setup_sources_from_entries(
            &t.fs,
            &t.paths,
            &[(
                "git-source",
                make_git_entry(
                    "https://github.com/example/repo.git",
                    clone_path.clone(),
                    "main",
                    None,
                ),
            )],
        );
        // Create the clone directory
        t.fs.add_file(clone_path.join("README.md"), "# repo");

        let ctx = t.ctx();
        remove("git-source", &ctx).unwrap();

        assert!(!t.fs.exists(&clone_path), "clone directory should be removed");
        let updated_sources = config::load_sources(&t.fs, &t.paths).unwrap();
        assert!(!updated_sources.sources.contains_key("git-source"));
    }

    #[test]
    fn remove_only_updates_json_for_local_source() {
        let t = TestContext::new();

        let local_dir = PathBuf::from("/local/repo");
        setup_sources_from_entries(
            &t.fs,
            &t.paths,
            &[("local-source", make_local_entry(local_dir.clone(), None))],
        );
        t.fs.add_dir(local_dir.clone());

        let ctx = t.ctx();
        remove("local-source", &ctx).unwrap();

        // Local dir should still exist (we only remove git clones)
        assert!(t.fs.exists(&local_dir), "local dir should not be removed");
        let updated_sources = config::load_sources(&t.fs, &t.paths).unwrap();
        assert!(!updated_sources.sources.contains_key("local-source"));
    }

    // --- failure-path tests ---

    #[test]
    fn add_git_source_does_not_save_entry_when_clone_fails() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient {
            cloned: RefCell::new(Vec::new()),
            pulled: RefCell::new(Vec::new()),
            should_fail: true,
        };
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_empty_sources(&fs, &paths);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = add("new-source", "https://github.com/example/repo.git", &ctx);
        assert!(result.is_err(), "expected Err when clone fails");

        // Sources file should remain empty — no partial save
        let sources = config::load_sources(&fs, &paths).unwrap();
        assert!(sources.sources.is_empty(), "sources should not be modified after failed clone");
    }
}
