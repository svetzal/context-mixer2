use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use crate::config;
use crate::context::AppContext;
use crate::gateway::{DirEntry, Filesystem};
use crate::scan;
use crate::source_iter;
use crate::source_update;
use crate::types::{Artifact, ArtifactKind, SourceEntry, SourceType, format_version_prefix};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub use crate::scan::ScanWarning;

pub struct SourceScanResult {
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

impl fmt::Display for SourceListResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.entries.is_empty() {
            return write!(
                f,
                "No sources registered.\n\nAdd one with: cmx source add <name> <path-or-url>\n"
            );
        }
        for entry in &self.entries {
            writeln!(f, "  {:<28} ({}) {}", entry.name, entry.kind, entry.location)?;
        }
        Ok(())
    }
}

impl fmt::Display for SourceBrowseResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.source_name;
        if self.agents.is_empty() && self.skills.is_empty() {
            return writeln!(f, "No agents or skills found in '{name}'.");
        }
        if !self.agents.is_empty() {
            writeln!(f, "Agents:")?;
            for a in &self.agents {
                let v = format_version_prefix(a.version.as_deref());
                writeln!(f, "  {}{v}{}", a.name, a.deprecation_display)?;
            }
        }
        if !self.skills.is_empty() {
            if !self.agents.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Skills:")?;
            for s in &self.skills {
                let v = format_version_prefix(s.version.as_deref());
                writeln!(f, "  {}{v}{}", s.name, s.deprecation_display)?;
                for file in &s.files {
                    writeln!(f, "    {file}")?;
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for SourceScanResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Source '{}' registered: {} agent(s), {} skill(s) found.",
            self.name, self.agents_found, self.skills_found
        )?;
        for warning in &self.warnings {
            writeln!(f, "Warning: {}", warning.message)?;
        }
        Ok(())
    }
}

impl fmt::Display for SourceRemoveResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.clone_deleted {
            writeln!(f, "Source '{}' removed (cloned repo deleted).", self.name)
        } else {
            writeln!(f, "Source '{}' removed.", self.name)
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn add_with(name: &str, path_or_url: &str, ctx: &AppContext<'_>) -> Result<SourceScanResult> {
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    if sources.sources.contains_key(name) {
        bail!("Source '{name}' already exists. Remove it first to re-register.");
    }

    let entry = if looks_like_url(path_or_url) {
        add_git_source_with(name, path_or_url, ctx)?
    } else {
        add_local_source_with(path_or_url, ctx)?
    };

    let (agents_found, skills_found, warnings) = scan_and_count(&entry, ctx.fs)?;

    config::mutate_sources_with(ctx.fs, ctx.paths, |sources| {
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
                location: location.unwrap_or_else(|| "<no location>".to_string()),
            }
        })
        .collect();

    Ok(SourceListResult { entries })
}

pub fn browse_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceBrowseResult> {
    source_update::auto_update_source_with(name, ctx)?;

    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;

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

    let all_artifacts = source_iter::each_source_artifact_with(&sources.sources, ctx.fs)?;
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

pub fn remove_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceRemoveResult> {
    let entry = config::mutate_sources_with(ctx.fs, ctx.paths, |sources| {
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

// ---------------------------------------------------------------------------
// Pure helpers (no I/O)
// ---------------------------------------------------------------------------

/// Build a `SourceBrowseResult` from pre-loaded data with no filesystem access.
fn build_browse_result(
    source_name: &str,
    artifacts: &[Artifact],
    skill_dirs: &HashMap<PathBuf, Vec<String>>,
) -> SourceBrowseResult {
    let agents = artifacts_of_kind(artifacts, ArtifactKind::Agent)
        .map(|a| BrowseArtifact {
            name: a.name.clone(),
            version: a.version.clone(),
            deprecation_display: format_deprecation(a),
        })
        .collect();

    let skills = artifacts_of_kind(artifacts, ArtifactKind::Skill)
        .map(|s| {
            let files = skill_dirs.get(&s.path).cloned().unwrap_or_default();
            build_browse_skill(s, files)
        })
        .collect();

    SourceBrowseResult {
        source_name: source_name.to_string(),
        agents,
        skills,
    }
}

fn artifacts_of_kind(
    artifacts: &[Artifact],
    kind: crate::types::ArtifactKind,
) -> impl Iterator<Item = &Artifact> {
    artifacts.iter().filter(move |a| a.kind == kind)
}

fn dir_entry_names(entries: &[DirEntry]) -> Vec<String> {
    let mut names: Vec<String> = entries
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
}

fn build_browse_skill(artifact: &Artifact, files: Vec<String>) -> BrowseSkill {
    BrowseSkill {
        name: artifact.name.clone(),
        version: artifact.version.clone(),
        deprecation_display: format_deprecation(artifact),
        files,
    }
}

fn count_artifacts(artifacts: &[Artifact]) -> (usize, usize) {
    let agents = artifacts_of_kind(artifacts, crate::types::ArtifactKind::Agent).count();
    let skills = artifacts_of_kind(artifacts, crate::types::ArtifactKind::Skill).count();
    (agents, skills)
}

pub(crate) fn scan_and_count(
    entry: &crate::types::SourceEntry,
    fs: &dyn Filesystem,
) -> Result<(usize, usize, Vec<ScanWarning>)> {
    let local_path = config::resolve_local_path(entry)?;
    let scan_result = scan::scan_source_with(&local_path, fs)?;
    let (agents_found, skills_found) = count_artifacts(&scan_result.artifacts);
    Ok((agents_found, skills_found, scan_result.warnings))
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
    use crate::scan::ScanWarning;
    use crate::test_support::{
        TestContext, make_ctx, make_git_entry, make_local_entry, setup_empty_sources,
        setup_sources_from_entries, test_paths,
    };
    use crate::types::{ArtifactKind, Deprecation};
    use chrono::Utc;
    use std::cell::RefCell;
    use std::path::PathBuf;

    // --- Display for SourceListResult ---

    #[test]
    fn source_list_result_display_empty() {
        let result = SourceListResult { entries: vec![] };
        let out = result.to_string();
        assert!(out.contains("No sources registered."));
        assert!(out.contains("cmx source add"));
    }

    #[test]
    fn source_list_result_display_with_entries() {
        let result = SourceListResult {
            entries: vec![SourceListEntry {
                name: "guidelines".to_string(),
                kind: "local",
                location: "/home/user/repos/guidelines".to_string(),
            }],
        };
        let out = result.to_string();
        assert!(out.contains("guidelines"));
        assert!(out.contains("local"));
        assert!(out.contains("/home/user/repos/guidelines"));
    }

    // --- Display for SourceBrowseResult ---

    #[test]
    fn source_browse_result_display_empty() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![],
            skills: vec![],
        };
        let out = result.to_string();
        assert!(out.contains("No agents or skills found in 'my-source'"));
    }

    #[test]
    fn source_browse_result_display_agents_only() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![BrowseArtifact {
                name: "rust-craftsperson".to_string(),
                version: Some("1.0.0".to_string()),
                deprecation_display: String::new(),
            }],
            skills: vec![],
        };
        let out = result.to_string();
        assert!(out.contains("Agents:"));
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("v1.0.0"));
        assert!(!out.contains("Skills:"));
    }

    #[test]
    fn source_browse_result_display_skills_only() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![],
            skills: vec![BrowseSkill {
                name: "my-skill".to_string(),
                version: None,
                deprecation_display: String::new(),
                files: vec!["tool.md".to_string()],
            }],
        };
        let out = result.to_string();
        assert!(!out.contains("Agents:"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("tool.md"));
    }

    #[test]
    fn source_browse_result_display_agents_and_skills() {
        let result = SourceBrowseResult {
            source_name: "my-source".to_string(),
            agents: vec![BrowseArtifact {
                name: "my-agent".to_string(),
                version: None,
                deprecation_display: String::new(),
            }],
            skills: vec![BrowseSkill {
                name: "my-skill".to_string(),
                version: Some("2.0.0".to_string()),
                deprecation_display: String::new(),
                files: vec![],
            }],
        };
        let out = result.to_string();
        assert!(out.contains("Agents:"));
        assert!(out.contains("my-agent"));
        assert!(out.contains("Skills:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("v2.0.0"));
    }

    // --- Display for SourceScanResult ---

    #[test]
    fn source_scan_result_display_no_warnings() {
        let result = SourceScanResult {
            name: "my-source".to_string(),
            agents_found: 3,
            skills_found: 1,
            warnings: vec![],
        };
        let out = result.to_string();
        assert!(out.contains("my-source"));
        assert!(out.contains("3 agent(s)"));
        assert!(!out.contains("Warning:"));
    }

    #[test]
    fn source_scan_result_display_with_warnings() {
        let result = SourceScanResult {
            name: "my-source".to_string(),
            agents_found: 0,
            skills_found: 0,
            warnings: vec![ScanWarning {
                message: "something fishy".to_string(),
            }],
        };
        let out = result.to_string();
        assert!(out.contains("Warning: something fishy"));
    }

    // --- Display for SourceRemoveResult ---

    #[test]
    fn source_remove_result_display_with_clone_deleted() {
        let result = SourceRemoveResult {
            name: "git-source".to_string(),
            clone_deleted: true,
        };
        let out = result.to_string();
        assert!(out.contains("git-source"));
        assert!(out.contains("cloned repo deleted"));
    }

    #[test]
    fn source_remove_result_display_without_clone() {
        let result = SourceRemoveResult {
            name: "local-source".to_string(),
            clone_deleted: false,
        };
        let out = result.to_string();
        assert!(out.contains("local-source"));
        assert!(!out.contains("cloned repo deleted"));
    }

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
        let result = add_with("my-source", "/new/path", &ctx);
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
        let result = add_with("local-source", "/local/repo", &ctx);
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
        let result = add_with("local-source", "/local/repo", &ctx).unwrap();

        assert_eq!(result.name, "local-source");
        assert_eq!(result.agents_found, 0, "empty repo has no agents");
        assert_eq!(result.skills_found, 0, "empty repo has no skills");
    }

    #[test]
    fn add_detects_url_and_clones() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = add_with("git-source", "https://github.com/example/repo.git", &ctx);
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
        add_with("new-source", "/local/repo", &ctx).unwrap();

        let sources = config::load_sources_with(&t.fs, &t.paths).unwrap();
        assert!(sources.sources.contains_key("new-source"), "source should be saved");
    }

    #[test]
    fn gather_list_empty_sources_returns_empty_entries() {
        let t = TestContext::new();

        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let result = list_with(&ctx).unwrap();

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
        let result = list_with(&ctx).unwrap();

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
        let result = remove_with("git-source", &ctx).unwrap();

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
        remove_with("git-source", &ctx).unwrap();

        assert!(!t.fs.exists(&clone_path), "clone directory should be removed");
        let updated_sources = config::load_sources_with(&t.fs, &t.paths).unwrap();
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
        remove_with("local-source", &ctx).unwrap();

        // Local dir should still exist (we only remove git clones)
        assert!(t.fs.exists(&local_dir), "local dir should not be removed");
        let updated_sources = config::load_sources_with(&t.fs, &t.paths).unwrap();
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
        let result = add_with("new-source", "https://github.com/example/repo.git", &ctx);
        assert!(result.is_err(), "expected Err when clone fails");

        // Sources file should remain empty — no partial save
        let sources = config::load_sources_with(&fs, &paths).unwrap();
        assert!(sources.sources.is_empty(), "sources should not be modified after failed clone");
    }

    // --- dir_entry_names ---

    fn make_dir_entry(file_name: &str, is_dir: bool) -> crate::gateway::DirEntry {
        crate::gateway::DirEntry {
            path: PathBuf::from(file_name),
            file_name: file_name.to_string(),
            is_dir,
        }
    }

    #[test]
    fn dir_entry_names_filters_dotfiles() {
        let entries = vec![
            make_dir_entry(".hidden", false),
            make_dir_entry("visible.md", false),
        ];
        let names = dir_entry_names(&entries);
        assert_eq!(names, vec!["visible.md"]);
    }

    #[test]
    fn dir_entry_names_appends_slash_to_dirs() {
        let entries = vec![
            make_dir_entry("subdir", true),
            make_dir_entry("file.md", false),
        ];
        let names = dir_entry_names(&entries);
        assert!(names.contains(&"subdir/".to_string()));
        assert!(names.contains(&"file.md".to_string()));
    }

    #[test]
    fn dir_entry_names_sorts_results() {
        let entries = vec![
            make_dir_entry("z.md", false),
            make_dir_entry("a.md", false),
            make_dir_entry("m.md", false),
        ];
        let names = dir_entry_names(&entries);
        assert_eq!(names, vec!["a.md", "m.md", "z.md"]);
    }

    // --- build_browse_result ---

    #[test]
    fn build_browse_result_separates_agents_and_skills() {
        let mut skill_dirs = HashMap::new();
        let skill_path = PathBuf::from("my-skill");
        skill_dirs.insert(skill_path.clone(), vec!["tool.md".to_string()]);

        let artifacts = vec![make_agent("alpha"), make_skill("my-skill")];
        let result = build_browse_result("test-source", &artifacts, &skill_dirs);

        assert_eq!(result.source_name, "test-source");
        assert_eq!(result.agents.len(), 1);
        assert_eq!(result.agents[0].name, "alpha");
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "my-skill");
        assert_eq!(result.skills[0].files, vec!["tool.md"]);
    }

    #[test]
    fn build_browse_result_empty_skill_dirs_gives_empty_files() {
        let artifacts = vec![make_skill("lonely-skill")];
        let result = build_browse_result("src", &artifacts, &HashMap::new());
        assert_eq!(result.skills[0].files, Vec::<String>::new());
    }

    // --- build_browse_skill ---

    #[test]
    fn build_browse_skill_populates_fields() {
        let artifact = make_skill("my-skill");
        let files = vec!["a.md".to_string(), "b.md".to_string()];
        let skill = build_browse_skill(&artifact, files.clone());
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.version, None);
        assert_eq!(skill.deprecation_display, "");
        assert_eq!(skill.files, files);
    }

    #[test]
    fn build_browse_skill_includes_version_and_deprecation() {
        let artifact = Artifact {
            kind: ArtifactKind::Skill,
            name: "my-skill".to_string(),
            description: String::new(),
            path: PathBuf::from("my-skill"),
            version: Some("1.2.3".to_string()),
            deprecation: Some(Deprecation {
                reason: Some("Old".to_string()),
                replacement: Some("new-skill".to_string()),
            }),
        };
        let skill = build_browse_skill(&artifact, vec![]);
        assert_eq!(skill.version, Some("1.2.3".to_string()));
        assert!(skill.deprecation_display.contains("DEPRECATED"));
    }
}
