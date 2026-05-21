use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};

use crate::config;
use crate::context::AppContext;
use crate::source::SourceScanResult;
use crate::types::{SourceEntry, SourceType};

const AUTO_UPDATE_MINUTES: i64 = 60;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub enum SourceUpdateOutput {
    SingleUpdate(SourceScanResult),
    BatchUpdate(Vec<SourceScanResult>),
    NoGitSources,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn update(name: Option<&str>, ctx: &AppContext<'_>) -> Result<SourceUpdateOutput> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;

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

pub(crate) fn perform_pull_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceScanResult> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;

    let source_entry = sources
        .sources
        .get(name)
        .with_context(|| format!("Source '{name}' not found."))?;
    let source_type = source_entry.source_type.clone();

    if matches!(source_type, SourceType::Git) {
        let clone_path = source_entry
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
    }

    let now = ctx.clock.now().to_rfc3339();
    let updated_entry = config::mutate_sources(ctx.fs, ctx.paths, |sources| {
        if let Some(entry) = sources.sources.get_mut(name) {
            entry.last_updated = Some(now);
        }
        sources
            .sources
            .get(name)
            .cloned()
            .with_context(|| format!("Source '{name}' not found."))
    })?;

    let (agents_found, skills_found, _) = crate::source::scan_and_count(&updated_entry, ctx.fs)?;

    Ok(SourceScanResult {
        name: name.to_string(),
        agents_found,
        skills_found,
        warnings: vec![],
    })
}

/// Auto-update a git source if it hasn't been updated recently.
pub fn auto_update_source(name: &str, ctx: &AppContext<'_>) -> Result<()> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;
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

/// Ensure sources are current before operating on them.
pub fn ensure_fresh(ctx: &AppContext<'_>) -> Result<()> {
    auto_update_all(ctx)
}

/// Auto-update all stale git sources.
pub fn auto_update_all(ctx: &AppContext<'_>) -> Result<()> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;
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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::source::SourceScanResult;
    use crate::test_support::{
        TestContext, make_ctx, make_git_entry, make_local_entry, setup_source_git,
        setup_sources_from_entries, test_paths,
    };
    use chrono::Utc;
    use std::cell::RefCell;
    use std::path::PathBuf;

    // --- Display for SourceUpdateOutput ---

    #[test]
    fn source_update_output_display_no_git_sources() {
        let out = SourceUpdateOutput::NoGitSources.to_string();
        assert_eq!(out, "No git-backed sources to update.\n");
    }

    #[test]
    fn source_update_output_display_single_update() {
        let out = SourceUpdateOutput::SingleUpdate(SourceScanResult {
            name: "guidelines".to_string(),
            agents_found: 5,
            skills_found: 3,
            warnings: vec![],
        })
        .to_string();
        assert!(out.contains("guidelines"));
        assert!(out.contains("5 agent(s)"));
        assert!(out.contains("3 skill(s)"));
    }

    #[test]
    fn source_update_output_display_batch_update() {
        let out = SourceUpdateOutput::BatchUpdate(vec![
            SourceScanResult {
                name: "source-a".to_string(),
                agents_found: 1,
                skills_found: 0,
                warnings: vec![],
            },
            SourceScanResult {
                name: "source-b".to_string(),
                agents_found: 2,
                skills_found: 4,
                warnings: vec![],
            },
        ])
        .to_string();
        assert!(out.contains("source-a"));
        assert!(out.contains("source-b"));
        assert!(out.contains("2 agent(s)"));
        assert!(out.contains("4 skill(s)"));
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

    // --- auto_update tests ---

    #[test]
    fn auto_update_skips_local_sources() {
        let t = TestContext::new();

        setup_sources_from_entries(
            &t.fs,
            &t.paths,
            &[("local-source", make_local_entry("/local/repo", None))],
        );

        let ctx = t.ctx();
        auto_update_source("local-source", &ctx).unwrap();

        assert!(t.git.pulled.borrow().is_empty(), "no git pull expected for local source");
    }

    #[test]
    fn auto_update_skips_fresh_git_sources() {
        let t = TestContext::new();

        let clone_path = PathBuf::from("/clones/git-source");
        setup_source_git(
            &t.fs,
            &t.paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            Some(Utc::now().to_rfc3339()),
        );
        t.fs.add_dir(clone_path);

        let ctx = t.ctx();
        auto_update_source("git-source", &ctx).unwrap();

        assert!(t.git.pulled.borrow().is_empty(), "fresh source should not be pulled");
    }

    #[test]
    fn auto_update_pulls_stale_git_sources() {
        let t = TestContext::new();

        let clone_path = PathBuf::from("/clones/git-source");
        let old_time = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        setup_source_git(
            &t.fs,
            &t.paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            Some(old_time),
        );
        t.fs.add_dir(clone_path.clone());

        let ctx = t.ctx();
        auto_update_source("git-source", &ctx).unwrap();

        let pulled = t.git.pulled.borrow();
        assert_eq!(pulled.len(), 1, "stale source should be pulled");
        assert_eq!(pulled[0], clone_path);
    }

    #[test]
    fn perform_pull_updates_timestamp_for_git_source() {
        let t = TestContext::new();

        let clone_path = PathBuf::from("/clones/git-source");
        setup_source_git(
            &t.fs,
            &t.paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            None,
        );
        t.fs.add_dir(clone_path);

        let ctx = t.ctx();
        let result = perform_pull_with("git-source", &ctx).unwrap();

        assert_eq!(result.name, "git-source");

        let updated_sources = crate::config::load_sources(&t.fs, &t.paths).unwrap();
        let entry = updated_sources.sources.get("git-source").unwrap();
        assert!(entry.last_updated.is_some(), "timestamp should be updated after pull");
    }

    #[test]
    fn perform_pull_does_not_update_timestamp_when_pull_fails() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient {
            cloned: RefCell::new(Vec::new()),
            pulled: RefCell::new(Vec::new()),
            should_fail: true,
        };
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        let original_timestamp = "2024-01-01T00:00:00Z".to_string();
        setup_source_git(
            &fs,
            &paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            Some(original_timestamp.clone()),
        );
        fs.add_dir(clone_path);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = perform_pull_with("git-source", &ctx);
        assert!(result.is_err(), "expected Err when pull fails");

        let updated_sources = crate::config::load_sources(&fs, &paths).unwrap();
        let entry = updated_sources.sources.get("git-source").unwrap();
        assert_eq!(
            entry.last_updated.as_deref(),
            Some(original_timestamp.as_str()),
            "timestamp should not change after failed pull"
        );
    }

    #[test]
    fn auto_update_all_stops_on_first_pull_failure() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient {
            cloned: RefCell::new(Vec::new()),
            pulled: RefCell::new(Vec::new()),
            should_fail: true,
        };
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let old_time = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let clone_a = PathBuf::from("/clones/source-a");
        let clone_b = PathBuf::from("/clones/source-b");

        setup_sources_from_entries(
            &fs,
            &paths,
            &[
                (
                    "source-a",
                    make_git_entry(
                        "https://github.com/example/a.git",
                        clone_a.clone(),
                        "main",
                        Some(old_time.clone()),
                    ),
                ),
                (
                    "source-b",
                    make_git_entry(
                        "https://github.com/example/b.git",
                        clone_b.clone(),
                        "main",
                        Some(old_time),
                    ),
                ),
            ],
        );
        fs.add_dir(clone_a);
        fs.add_dir(clone_b);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = auto_update_all(&ctx);
        assert!(result.is_err(), "expected Err when any pull fails");

        let pulled = git.pulled.borrow();
        assert!(
            pulled.is_empty(),
            "no pull should complete when all pulls are configured to fail"
        );
    }
}
