use anyhow::Result;
use std::fmt;

use crate::context::AppContext;
use crate::source_iter;
use crate::source_update;
use crate::table::Table;
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

impl fmt::Display for SearchOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let query = &self.query;
        let results = &self.results;

        if results.is_empty() {
            return writeln!(f, "No results for '{query}'.");
        }

        let table = Table {
            headers: vec!["Name", "Type", "Version", "Source", "Description"],
            padded_cols: 4,
            rows: results
                .iter()
                .map(|r| {
                    vec![
                        r.name.clone(),
                        r.kind.clone(),
                        r.version.clone(),
                        r.source.clone(),
                        r.description.clone(),
                    ]
                })
                .collect(),
        }
        .render();

        write!(f, "{table}\n{} result(s) found.\n", results.len())
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn search(query: &str, ctx: &AppContext<'_>) -> Result<SearchOutput> {
    source_update::ensure_fresh(ctx)?;

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
    use crate::test_support::{TestContext, setup_source_with_agent};

    // --- Display for SearchOutput ---

    #[test]
    fn search_output_display_empty() {
        let output = SearchOutput {
            query: "foo".to_string(),
            results: vec![],
        };
        assert_eq!(output.to_string(), "No results for 'foo'.\n");
    }

    #[test]
    fn search_output_display_with_data() {
        let output = SearchOutput {
            query: "rust".to_string(),
            results: vec![SearchResult {
                name: "rust-craftsperson".to_string(),
                kind: "agent".to_string(),
                version: "1.0.0".to_string(),
                source: "guidelines".to_string(),
                description: "Rust expert".to_string(),
            }],
        };
        let out = output.to_string();
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("1 result(s) found."));
    }

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
        let t = TestContext::new();

        setup_source_with_agent(
            &t.fs,
            &t.paths,
            "my-source",
            "/sources/my-source",
            "rust-craftsperson",
        );

        let ctx = t.ctx();
        let output = search("rust", &ctx).unwrap();

        assert_eq!(output.query, "rust");
        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].name, "rust-craftsperson");
        assert_eq!(output.results[0].source, "my-source");
    }

    #[test]
    fn gather_search_results_matches_by_description() {
        let t = TestContext::new();

        // agent_content uses "A test agent" as description
        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let output = search("test agent", &ctx).unwrap();

        assert_eq!(output.results.len(), 1);
        assert_eq!(output.results[0].name, "my-agent");
    }

    #[test]
    fn gather_search_results_no_match_returns_empty() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let output = search("nonexistent-xyz", &ctx).unwrap();

        assert!(output.results.is_empty(), "expected no results for non-matching query");
    }

    #[test]
    fn gather_search_results_case_insensitive() {
        let t = TestContext::new();

        setup_source_with_agent(&t.fs, &t.paths, "my-source", "/sources/my-source", "my-agent");

        let ctx = t.ctx();
        let output = search("MY-AGENT", &ctx).unwrap();

        assert_eq!(output.results.len(), 1, "search should be case-insensitive");
    }
}
