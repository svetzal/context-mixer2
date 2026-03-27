use anyhow::Result;

use crate::config;
use crate::context::AppContext;
use crate::source;
use crate::source_iter;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

pub struct SearchResult {
    pub name: String,
    pub kind: String,
    pub version: String,
    pub source: String,
    pub description: String,
}

pub struct SearchOutput {
    pub results: Vec<SearchResult>,
    pub query: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn search_with(query: &str, ctx: &AppContext<'_>) -> Result<SearchOutput> {
    gather_search_results_with(query, ctx)
}

// ---------------------------------------------------------------------------
// Gather (pure logic, no println!)
// ---------------------------------------------------------------------------

pub(crate) fn gather_search_results_with(
    query: &str,
    ctx: &AppContext<'_>,
) -> Result<SearchOutput> {
    source::auto_update_all_with(ctx)?;

    let query_lower = query.to_lowercase();
    let sources = config::load_sources_with(ctx.fs, ctx.paths)?;
    let mut results = Vec::new();

    for sa in source_iter::each_source_artifact_with(&sources.sources, ctx.fs) {
        let name_lower = sa.artifact.name.to_lowercase();
        let desc_lower = sa.artifact.description.to_lowercase();

        if name_lower.contains(&query_lower) || desc_lower.contains(&query_lower) {
            let short_desc = truncate_description(&sa.artifact.description, 80);

            results.push(SearchResult {
                name: sa.artifact.name,
                kind: sa.artifact.kind.to_string(),
                version: sa.artifact.version.as_deref().unwrap_or("-").to_string(),
                source: sa.source_name,
                description: short_desc,
            });
        }
    }

    Ok(SearchOutput {
        results,
        query: query.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Print (no business logic)
// ---------------------------------------------------------------------------

pub fn print_search_results(output: &SearchOutput) {
    let query = &output.query;
    let results = &output.results;

    if results.is_empty() {
        println!("No results for '{query}'.");
        return;
    }

    let w_name = results.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let w_kind = 5;
    let w_ver = results.iter().map(|r| r.version.len()).max().unwrap_or(7).max(7);
    let w_src = results.iter().map(|r| r.source.len()).max().unwrap_or(6).max(6);

    println!(
        "  {:<w_name$}  {:<w_kind$}  {:<w_ver$}  {:<w_src$}  Description",
        "Name", "Type", "Version", "Source",
    );
    println!(
        "  {:<w_name$}  {:<w_kind$}  {:<w_ver$}  {:<w_src$}  -----------",
        "-".repeat(w_name),
        "-".repeat(w_kind),
        "-".repeat(w_ver),
        "-".repeat(w_src),
    );

    for r in results {
        println!(
            "  {:<w_name$}  {:<w_kind$}  {:<w_ver$}  {:<w_src$}  {}",
            r.name, r.kind, r.version, r.source, r.description,
        );
    }

    println!();
    println!("{} result(s) found.", results.len());
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

fn truncate_description(desc: &str, max_len: usize) -> String {
    // Take the first line or sentence, handling escaped \n
    let first_part = desc
        .split("\\n")
        .next()
        .unwrap_or(desc)
        .split('\n')
        .next()
        .unwrap_or(desc)
        .trim();

    if first_part.len() <= max_len {
        first_part.to_string()
    } else {
        format!("{}...", &first_part[..max_len - 3])
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{make_ctx, setup_source_with_agent, test_paths};
    use chrono::Utc;

    // --- truncate_description ---

    #[test]
    fn truncate_description_short_returned_as_is() {
        let s = "Short description";
        assert_eq!(truncate_description(s, 80), s);
    }

    #[test]
    fn truncate_description_exactly_at_limit_returned_as_is() {
        let s = "a".repeat(80);
        assert_eq!(truncate_description(&s, 80), s);
    }

    #[test]
    fn truncate_description_long_gets_ellipsis() {
        let s = "a".repeat(100);
        let result = truncate_description(&s, 80);
        assert!(result.ends_with("..."));
        assert_eq!(result.len(), 80);
    }

    #[test]
    fn truncate_description_newline_takes_first_line() {
        let s = "First line\nSecond line\nThird line";
        assert_eq!(truncate_description(s, 80), "First line");
    }

    #[test]
    fn truncate_description_escaped_newline_takes_first_part() {
        let s = "First part\\nSecond part";
        assert_eq!(truncate_description(s, 80), "First part");
    }

    #[test]
    fn truncate_description_empty_string() {
        assert_eq!(truncate_description("", 80), "");
    }

    #[test]
    fn truncate_description_trims_whitespace() {
        let s = "  leading and trailing  ";
        assert_eq!(truncate_description(s, 80), "leading and trailing");
    }

    // --- gather_search_results_with ---

    #[test]
    fn gather_search_results_matches_by_name() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(
            &fs,
            &paths,
            "my-source",
            "/sources/my-source",
            "rust-craftsperson",
        );

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let output = gather_search_results_with("rust", &ctx).unwrap();

        assert_eq!(output.query, "rust");
        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].name, "rust-craftsperson");
        assert_eq!(output.results[0].source, "my-source");
    }

    #[test]
    fn gather_search_results_matches_by_description() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        // agent_content uses "A test agent" as description
        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let output = gather_search_results_with("test agent", &ctx).unwrap();

        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].name, "my-agent");
    }

    #[test]
    fn gather_search_results_no_match_returns_empty() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let output = gather_search_results_with("nonexistent-xyz", &ctx).unwrap();

        assert!(output.results.is_empty(), "expected no results for non-matching query");
    }

    #[test]
    fn gather_search_results_case_insensitive() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        setup_source_with_agent(&fs, &paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = make_ctx(&fs, &git, &clock, &paths);
        let output = gather_search_results_with("MY-AGENT", &ctx).unwrap();

        assert_eq!(output.results.len(), 1, "search should be case-insensitive");
    }
}
