use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

use crate::config;
use crate::context::AppContext;
use crate::scan;
use crate::source_iter;
use crate::types::{Artifact, SourceEntry, SourceType};

const AUTO_UPDATE_MINUTES: i64 = 60;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub use crate::scan::ScanWarning;

pub struct SourceAddResult {
    pub name: String,
    pub agents_found: usize,
    pub skills_found: usize,
    pub warnings: Vec<ScanWarning>,
}

pub struct SourceListEntry {
    pub name: String,
    pub kind: &'static str,
    pub location: String,
}

pub struct SourceListResult {
    pub entries: Vec<SourceListEntry>,
}

pub struct BrowseArtifact {
    pub name: String,
    pub version: Option<String>,
    pub deprecation_display: String,
}

pub struct BrowseSkill {
    pub name: String,
    pub version: Option<String>,
    pub deprecation_display: String,
    pub files: Vec<String>,
}

pub struct SourceBrowseResult {
    pub source_name: String,
    pub agents: Vec<BrowseArtifact>,
    pub skills: Vec<BrowseSkill>,
}

pub struct SourceRemoveResult {
    pub name: String,
    pub clone_deleted: bool,
}

pub struct SourceUpdateResult {
    pub name: String,
    pub agents_found: usize,
    pub skills_found: usize,
}

pub enum SourceUpdateOutput {
    SingleUpdate(SourceUpdateResult),
    BatchUpdate(Vec<SourceUpdateResult>),
    NoGitSources,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn update_with(name: Option<&str>, ctx: &AppContext<'_>) -> Result<SourceUpdateOutput> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    if let Some(n) = name {
        if !sources.sources.contains_key(n) {
            bail!("Source '{n}' not found.");
        }
        let result = perform_pull_with(n, ctx)?;
        Ok(SourceUpdateOutput::SingleUpdate(result))
    } else {
        let git_sources: Vec<_> = sources
            .sources
            .iter()
            .filter(|(_, e)| matches!(e.source_type, SourceType::Git))
            .map(|(n, _)| n.clone())
            .collect();

        if git_sources.is_empty() {
            return Ok(SourceUpdateOutput::NoGitSources);
        }

        let mut results = Vec::new();
        for source_name in &git_sources {
            let result = perform_pull_with(source_name, ctx)?;
            results.push(result);
        }
        Ok(SourceUpdateOutput::BatchUpdate(results))
    }
}

pub fn add_with(name: &str, path_or_url: &str, ctx: &AppContext<'_>) -> Result<SourceAddResult> {
    let mut sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    if sources.sources.contains_key(name) {
        bail!("Source '{name}' already exists. Remove it first to re-register.");
    }

    let entry = if looks_like_url(path_or_url) {
        add_git_source_with(name, path_or_url, ctx)?
    } else {
        add_local_source_with(path_or_url, ctx)?
    };

    let local_path = config::resolve_local_path(&entry);
    let scan_result = scan::scan_source_with(&local_path, ctx.fs)?;
    let (agents_found, skills_found) = count_artifacts(&scan_result.artifacts);

    sources.sources.insert(name.to_string(), entry);
    config::save_sources_with(&sources, ctx.fs, ctx.paths)?;

    Ok(SourceAddResult {
        name: name.to_string(),
        agents_found,
        skills_found,
        warnings: scan_result.warnings,
    })
}

pub fn list_with(ctx: &AppContext<'_>) -> Result<SourceListResult> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

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
                location: location.unwrap_or_default(),
            }
        })
        .collect();

    Ok(SourceListResult { entries })
}

pub fn browse_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceBrowseResult> {
    auto_update_source_with(name, ctx)?;

    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    let entry = sources.sources.get(name).with_context(|| {
        format!("Source '{name}' not found. Run 'cmx source list' to see registered sources.")
    })?;

    let local_path = config::resolve_local_path(entry);
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

    let all_artifacts = source_iter::each_source_artifact_with(&sources.sources, ctx.fs);
    let artifacts: Vec<_> = all_artifacts
        .into_iter()
        .filter(|sa| sa.source_name == name)
        .map(|sa| sa.artifact)
        .collect();

    let agents = artifacts
        .iter()
        .filter(|a| a.kind == crate::types::ArtifactKind::Agent)
        .map(|a| BrowseArtifact {
            name: a.name.clone(),
            version: a.version.clone(),
            deprecation_display: format_deprecation(a),
        })
        .collect();

    let skills = artifacts
        .iter()
        .filter(|a| a.kind == crate::types::ArtifactKind::Skill)
        .map(|s| {
            let files = if let Ok(entries) = ctx.fs.read_dir(&s.path) {
                let mut names: Vec<_> = entries
                    .iter()
                    .filter(|e| !e.file_name.starts_with('.'))
                    .map(|e| {
                        if e.is_dir {
                            format!("{}/", e.file_name)
                        } else {
                            e.file_name.clone()
                        }
                    })
                    .collect();
                names.sort();
                names
            } else {
                Vec::new()
            };

            BrowseSkill {
                name: s.name.clone(),
                version: s.version.clone(),
                deprecation_display: format_deprecation(s),
                files,
            }
        })
        .collect();

    Ok(SourceBrowseResult {
        source_name: name.to_string(),
        agents,
        skills,
    })
}

pub fn remove_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceRemoveResult> {
    let mut sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    let entry = sources
        .sources
        .remove(name)
        .with_context(|| format!("Source '{name}' not found."))?;

    config::save_sources_with(&sources, ctx.fs, ctx.paths)?;

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

pub(crate) fn perform_pull_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceUpdateResult> {
    let mut sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    // Clone or borrow what we need before mutating the map
    let source_type = sources
        .sources
        .get(name)
        .with_context(|| format!("Source '{name}' not found."))?
        .source_type
        .clone();

    match source_type {
        SourceType::Local => {
            // Update timestamp for local sources
            if let Some(entry) = sources.sources.get_mut(name) {
                entry.last_updated = Some(ctx.clock.now().to_rfc3339());
            }
            config::save_sources_with(&sources, ctx.fs, ctx.paths)?;
            let local_path =
                config::resolve_local_path(sources.sources.get(name).expect("entry present"));
            let scan_result = scan::scan_source_with(&local_path, ctx.fs)?;
            let (agents_found, skills_found) = count_artifacts(&scan_result.artifacts);
            return Ok(SourceUpdateResult {
                name: name.to_string(),
                agents_found,
                skills_found,
            });
        }
        SourceType::Git => {}
    }

    let clone_path = sources
        .sources
        .get(name)
        .expect("entry present")
        .local_clone
        .as_ref()
        .context("Git source has no local clone path")?
        .clone();

    if !ctx.fs.exists(&clone_path) {
        bail!(
            "Clone directory {} does not exist. Try removing and re-adding the source.",
            clone_path.display()
        );
    }

    ctx.git.pull(&clone_path)?;

    // Update timestamp
    if let Some(entry) = sources.sources.get_mut(name) {
        entry.last_updated = Some(ctx.clock.now().to_rfc3339());
    }
    config::save_sources_with(&sources, ctx.fs, ctx.paths)?;

    let local_path = config::resolve_local_path(sources.sources.get(name).expect("entry present"));
    let scan_result = scan::scan_source_with(&local_path, ctx.fs)?;
    let (agents_found, skills_found) = count_artifacts(&scan_result.artifacts);

    Ok(SourceUpdateResult {
        name: name.to_string(),
        agents_found,
        skills_found,
    })
}

// ---------------------------------------------------------------------------
// Auto-update helpers (no change in logic)
// ---------------------------------------------------------------------------

/// Auto-update a git source if it hasn't been updated recently.
pub fn auto_update_source_with(name: &str, ctx: &AppContext<'_>) -> Result<()> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let Some(entry) = sources.sources.get(name) else {
        return Ok(());
    };

    if !matches!(entry.source_type, SourceType::Git) {
        return Ok(());
    }

    if is_stale_at(entry, ctx.clock.now()) {
        perform_pull_with(name, ctx)?;
    }

    Ok(())
}

/// Auto-update all stale git sources.
pub fn auto_update_all_with(ctx: &AppContext<'_>) -> Result<()> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let now = ctx.clock.now();
    let stale_names: Vec<String> = sources
        .sources
        .iter()
        .filter(|(_, entry)| {
            matches!(entry.source_type, SourceType::Git) && is_stale_at(entry, now)
        })
        .map(|(name, _)| name.clone())
        .collect();
    for name in stale_names {
        perform_pull_with(&name, ctx)?;
    }
    Ok(())
}

fn is_stale_at(entry: &SourceEntry, now: DateTime<Utc>) -> bool {
    let Some(last) = &entry.last_updated else {
        return true;
    };
    let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    let age = now.signed_duration_since(last_time);
    age.num_minutes() >= AUTO_UPDATE_MINUTES
}

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

// ---------------------------------------------------------------------------
// Pure helpers (no I/O)
// ---------------------------------------------------------------------------

fn count_artifacts(artifacts: &[Artifact]) -> (usize, usize) {
    let agents = artifacts.iter().filter(|a| a.kind == crate::types::ArtifactKind::Agent).count();
    let skills = artifacts.iter().filter(|a| a.kind == crate::types::ArtifactKind::Skill).count();
    (agents, skills)
}

fn format_deprecation(artifact: &Artifact) -> String {
    let Some(dep) = &artifact.deprecation else {
        return String::new();
    };

    let mut parts = vec!["  ⛔ DEPRECATED".to_string()];

    if let Some(reason) = &dep.reason {
        parts.push(format!(": {reason}"));
    }

    if let Some(replacement) = &dep.replacement {
        parts.push(format!(" (use {replacement} instead)"));
    }

    parts.join("")
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
    use crate::gateway::Filesystem;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::paths::ConfigPaths;
    use crate::test_support::{make_ctx, make_git_entry, make_local_entry, test_paths};
    use crate::types::{ArtifactKind, Deprecation};
    use chrono::Utc;
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

    // --- is_stale_at ---

    #[test]
    fn is_stale_never_updated() {
        let entry = make_local_entry("/some/path", None);
        assert!(is_stale_at(&entry, Utc::now()));
    }

    #[test]
    fn is_stale_recent_update_is_fresh() {
        let now = Utc::now();
        let entry = make_local_entry("/some/path", Some(now.to_rfc3339()));
        assert!(!is_stale_at(&entry, now));
    }

    #[test]
    fn is_stale_old_update_is_stale() {
        let now = Utc::now();
        let old = (now - chrono::Duration::hours(2)).to_rfc3339();
        let entry = make_local_entry("/some/path", Some(old));
        assert!(is_stale_at(&entry, now));
    }

    #[test]
    fn is_stale_invalid_timestamp_is_stale() {
        let entry = make_local_entry("/some/path", Some("not-a-timestamp".to_string()));
        assert!(is_stale_at(&entry, Utc::now()));
    }

    // --- count_artifacts ---

    fn make_agent(name: &str) -> Artifact {
        Artifact {
            kind: ArtifactKind::Agent,
            name: name.to_string(),
            description: String::new(),
            path: PathBuf::from(format!("{name}.md")),
            version: None,
            deprecation: None,
        }
    }

    fn make_skill(name: &str) -> Artifact {
        Artifact {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            description: String::new(),
            path: PathBuf::from(name),
            version: None,
            deprecation: None,
        }
    }

    #[test]
    fn count_artifacts_empty() {
        assert_eq!(count_artifacts(&[]), (0, 0));
    }

    #[test]
    fn count_artifacts_only_agents() {
        let arts = vec![make_agent("alpha"), make_agent("beta")];
        assert_eq!(count_artifacts(&arts), (2, 0));
    }

    #[test]
    fn count_artifacts_mixed() {
        let arts = vec![make_agent("alpha"), make_skill("zap"), make_skill("zip")];
        assert_eq!(count_artifacts(&arts), (1, 2));
    }

    // --- format_deprecation ---

    #[test]
    fn format_deprecation_not_deprecated() {
        let artifact = make_agent("alpha");
        assert_eq!(format_deprecation(&artifact), "");
    }

    #[test]
    fn format_deprecation_deprecated_no_extras() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: None,
                replacement: None,
            }),
        };
        assert_eq!(format_deprecation(&artifact), "  ⛔ DEPRECATED");
    }

    #[test]
    fn format_deprecation_deprecated_with_reason() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: Some("Too old".to_string()),
                replacement: None,
            }),
        };
        assert_eq!(format_deprecation(&artifact), "  ⛔ DEPRECATED: Too old");
    }

    #[test]
    fn format_deprecation_deprecated_with_reason_and_replacement() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: Some("Too old".to_string()),
                replacement: Some("new-agent".to_string()),
            }),
        };
        assert_eq!(
            format_deprecation(&artifact),
            "  ⛔ DEPRECATED: Too old (use new-agent instead)"
        );
    }

    #[test]
    fn format_deprecation_deprecated_with_replacement_only() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: None,
                replacement: Some("new-agent".to_string()),
            }),
        };
        assert_eq!(format_deprecation(&artifact), "  ⛔ DEPRECATED (use new-agent instead)");
    }

    // --- source management business logic tests ---

    fn setup_empty_sources(fs: &FakeFilesystem, paths: &ConfigPaths) {
        let sources = crate::types::SourcesFile::default();
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
    }

    #[test]
    fn add_bails_when_source_name_already_exists() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // Pre-populate with existing source
        let mut sources = crate::types::SourcesFile::default();
        sources
            .sources
            .insert("my-source".to_string(), make_local_entry("/existing", None));
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = add_with("my-source", "/new/path", &ctx);
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("already exists"), "unexpected: {msg}");
    }

    #[test]
    fn add_detects_local_path_no_git_call() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_empty_sources(&fs, &paths);

        // Set up a valid local directory
        fs.add_dir("/local/repo");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = add_with("local-source", "/local/repo", &ctx);
        assert!(result.is_ok(), "expected ok: {:?}", result.err());

        // No git clone should have been called
        assert!(git.cloned.borrow().is_empty(), "no git clone expected for local path");
    }

    #[test]
    fn add_result_has_correct_name_and_counts() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_empty_sources(&fs, &paths);
        fs.add_dir("/local/repo");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = add_with("local-source", "/local/repo", &ctx).unwrap();

        assert_eq!(result.name, "local-source");
        assert_eq!(result.agents_found, 0, "empty repo has no agents");
        assert_eq!(result.skills_found, 0, "empty repo has no skills");
    }

    #[test]
    fn add_detects_url_and_clones() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_empty_sources(&fs, &paths);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = add_with("git-source", "https://github.com/example/repo.git", &ctx);
        assert!(result.is_ok(), "expected ok: {:?}", result.err());

        let cloned = git.cloned.borrow();
        assert_eq!(cloned.len(), 1, "expected one git clone");
        assert_eq!(cloned[0].0, "https://github.com/example/repo.git");
    }

    #[test]
    fn add_saves_sources_after_registration() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_empty_sources(&fs, &paths);
        fs.add_dir("/local/repo");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        add_with("new-source", "/local/repo", &ctx).unwrap();

        let sources = config::load_sources_with(&fs, &paths).unwrap();
        assert!(sources.sources.contains_key("new-source"), "source should be saved");
    }

    #[test]
    fn gather_list_empty_sources_returns_empty_entries() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_empty_sources(&fs, &paths);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = list_with(&ctx).unwrap();

        assert!(result.entries.is_empty(), "expected empty entries for no sources");
    }

    #[test]
    fn gather_list_local_source_has_correct_kind_and_location() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let mut sources = crate::types::SourcesFile::default();
        sources
            .sources
            .insert("my-source".to_string(), make_local_entry("/local/repo", None));
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = list_with(&ctx).unwrap();

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "my-source");
        assert_eq!(result.entries[0].kind, "local");
        assert_eq!(result.entries[0].location, "/local/repo");
    }

    #[test]
    fn remove_result_reports_clone_deleted() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        let mut sources = crate::types::SourcesFile::default();
        sources.sources.insert(
            "git-source".to_string(),
            make_git_entry("https://github.com/example/repo.git", clone_path.clone(), "main", None),
        );
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_file(clone_path.join("README.md"), "# repo");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = remove_with("git-source", &ctx).unwrap();

        assert_eq!(result.name, "git-source");
        assert!(result.clone_deleted, "expected clone_deleted to be true");
        assert!(!fs.exists(&clone_path), "clone directory should be removed");
    }

    #[test]
    fn remove_deletes_clone_directory_for_git_source() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        let mut sources = crate::types::SourcesFile::default();
        sources.sources.insert(
            "git-source".to_string(),
            make_git_entry("https://github.com/example/repo.git", clone_path.clone(), "main", None),
        );
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        // Create the clone directory
        fs.add_file(clone_path.join("README.md"), "# repo");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        remove_with("git-source", &ctx).unwrap();

        assert!(!fs.exists(&clone_path), "clone directory should be removed");
        let updated_sources = config::load_sources_with(&fs, &paths).unwrap();
        assert!(!updated_sources.sources.contains_key("git-source"));
    }

    #[test]
    fn remove_only_updates_json_for_local_source() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let local_dir = PathBuf::from("/local/repo");
        let mut sources = crate::types::SourcesFile::default();
        sources
            .sources
            .insert("local-source".to_string(), make_local_entry(local_dir.clone(), None));
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_dir(local_dir.clone());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        remove_with("local-source", &ctx).unwrap();

        // Local dir should still exist (we only remove git clones)
        assert!(fs.exists(&local_dir), "local dir should not be removed");
        let updated_sources = config::load_sources_with(&fs, &paths).unwrap();
        assert!(!updated_sources.sources.contains_key("local-source"));
    }

    #[test]
    fn auto_update_skips_local_sources() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let mut sources = crate::types::SourcesFile::default();
        // stale, but local
        sources
            .sources
            .insert("local-source".to_string(), make_local_entry("/local/repo", None));
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        auto_update_source_with("local-source", &ctx).unwrap();

        // No git pull should have been called
        assert!(git.pulled.borrow().is_empty(), "no git pull expected for local source");
    }

    #[test]
    fn auto_update_skips_fresh_git_sources() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        let mut sources = crate::types::SourcesFile::default();
        // Fresh — updated right now
        sources.sources.insert(
            "git-source".to_string(),
            make_git_entry(
                "https://github.com/example/repo.git",
                clone_path.clone(),
                "main",
                Some(Utc::now().to_rfc3339()),
            ),
        );
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_dir(clone_path);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        auto_update_source_with("git-source", &ctx).unwrap();

        assert!(git.pulled.borrow().is_empty(), "fresh source should not be pulled");
    }

    #[test]
    fn auto_update_pulls_stale_git_sources() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        let mut sources = crate::types::SourcesFile::default();
        let old_time = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        sources.sources.insert(
            "git-source".to_string(),
            make_git_entry(
                "https://github.com/example/repo.git",
                clone_path.clone(),
                "main",
                Some(old_time),
            ),
        );
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_dir(clone_path.clone());

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        auto_update_source_with("git-source", &ctx).unwrap();

        let pulled = git.pulled.borrow();
        assert_eq!(pulled.len(), 1, "stale source should be pulled");
        assert_eq!(pulled[0], clone_path);
    }

    #[test]
    fn perform_pull_updates_timestamp_for_git_source() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        let mut sources = crate::types::SourcesFile::default();
        sources.sources.insert(
            "git-source".to_string(),
            make_git_entry("https://github.com/example/repo.git", clone_path.clone(), "main", None),
        );
        fs.add_file(paths.sources_path(), serde_json::to_string_pretty(&sources).unwrap());
        fs.add_dir(clone_path);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = perform_pull_with("git-source", &ctx).unwrap();

        assert_eq!(result.name, "git-source");

        let updated_sources = config::load_sources_with(&fs, &paths).unwrap();
        let entry = updated_sources.sources.get("git-source").unwrap();
        assert!(entry.last_updated.is_some(), "timestamp should be updated after pull");
    }
}
