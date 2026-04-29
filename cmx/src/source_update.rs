use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};

use crate::config;
use crate::context::AppContext;
use crate::types::{SourceEntry, SourceType};

const AUTO_UPDATE_MINUTES: i64 = 60;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

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

pub(crate) fn perform_pull_with(name: &str, ctx: &AppContext<'_>) -> Result<SourceUpdateResult> {
    let mut sources = config::load_sources_with(ctx.fs, ctx.paths)?;

    let source_type = sources
        .sources
        .get(name)
        .with_context(|| format!("Source '{name}' not found."))?
        .source_type
        .clone();

    if matches!(source_type, SourceType::Git) {
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
    }

    if let Some(entry) = sources.sources.get_mut(name) {
        entry.last_updated = Some(ctx.clock.now().to_rfc3339());
    }
    config::save_sources_with(&sources, ctx.fs, ctx.paths)?;

    let entry = sources.sources.get(name).expect("entry present");
    let (agents_found, skills_found, _) = crate::source::scan_and_count(entry, ctx.fs)?;

    Ok(SourceUpdateResult {
        name: name.to_string(),
        agents_found,
        skills_found,
    })
}

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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{
        make_ctx, make_git_entry, make_local_entry, setup_source_git, setup_sources_from_entries,
        test_paths,
    };
    use chrono::Utc;
    use std::cell::RefCell;
    use std::path::PathBuf;

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
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_sources_from_entries(
            &fs,
            &paths,
            &[("local-source", make_local_entry("/local/repo", None))],
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        auto_update_source_with("local-source", &ctx).unwrap();

        assert!(git.pulled.borrow().is_empty(), "no git pull expected for local source");
    }

    #[test]
    fn auto_update_skips_fresh_git_sources() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let clone_path = PathBuf::from("/clones/git-source");
        setup_source_git(
            &fs,
            &paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            Some(Utc::now().to_rfc3339()),
        );
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
        let old_time = (Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        setup_source_git(
            &fs,
            &paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            Some(old_time),
        );
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
        setup_source_git(
            &fs,
            &paths,
            "git-source",
            "https://github.com/example/repo.git",
            clone_path.clone(),
            "main",
            None,
        );
        fs.add_dir(clone_path);

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let result = perform_pull_with("git-source", &ctx).unwrap();

        assert_eq!(result.name, "git-source");

        let updated_sources = crate::config::load_sources_with(&fs, &paths).unwrap();
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

        let updated_sources = crate::config::load_sources_with(&fs, &paths).unwrap();
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
        let result = auto_update_all_with(&ctx);
        assert!(result.is_err(), "expected Err when any pull fails");

        let pulled = git.pulled.borrow();
        assert!(
            pulled.is_empty(),
            "no pull should complete when all pulls are configured to fail"
        );
    }
}
