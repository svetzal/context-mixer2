use anyhow::Result;

use crate::context::AppContext;
use crate::source_iter;
use crate::source_update;
use crate::types::display_version;

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
// Public API
// ---------------------------------------------------------------------------

pub fn search_with(query: &str, ctx: &AppContext<'_>) -> Result<SearchOutput> {
    source_update::auto_update_all_with(ctx)?;

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for sa in source_iter::all_artifacts(ctx)? {
        let name_lower = sa.artifact.name.to_lowercase();
        let desc_lower = sa.artifact.description.to_lowercase();

        if name_lower.contains(&query_lower) || desc_lower.contains(&query_lower) {
            let short_desc = truncate_description(&sa.artifact.description, 80);

            results.push(SearchResult {
                name: sa.artifact.name,
                kind: sa.artifact.kind.to_string(),
                version: display_version(sa.artifact.version.as_deref()).to_string(),
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

    // --- search_with ---

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
        let output = search_with("rust", &ctx).unwrap();

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
        let output = search_with("test agent", &ctx).unwrap();

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
        let output = search_with("nonexistent-xyz", &ctx).unwrap();

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
        let output = search_with("MY-AGENT", &ctx).unwrap();

        assert_eq!(output.results.len(), 1, "search should be case-insensitive");
    }
}
