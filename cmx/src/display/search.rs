use std::fmt;

use crate::search::SearchOutput;

use super::util;

impl fmt::Display for SearchOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let query = &self.query;
        let results = &self.results;

        let rows: Vec<Vec<String>> = results
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
            .collect();

        let table = util::table_or_empty(
            &format!("No results for '{query}'."),
            vec!["Name", "Type", "Version", "Source", "Description"],
            4,
            rows,
        );

        if results.is_empty() {
            return write!(f, "{table}");
        }

        write!(f, "{table}\n{} result(s) found.\n", results.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchResult;

    // --- Step 9: SearchOutput ---

    #[test]
    fn search_output_empty_no_results_message() {
        let r = SearchOutput {
            query: "my-query".to_string(),
            results: vec![],
        };
        assert_eq!(r.to_string(), "No results for 'my-query'.\n");
    }

    #[test]
    fn search_output_populated_result_count() {
        let r = SearchOutput {
            query: "rust".to_string(),
            results: vec![SearchResult {
                name: "rust-craftsperson".to_string(),
                kind: "agent".to_string(),
                version: "1.0.0".to_string(),
                source: "guidelines".to_string(),
                description: "Rust expert".to_string(),
            }],
        };
        let out = r.to_string();
        assert!(out.contains("rust-craftsperson"));
        assert!(out.contains("1 result(s) found."));
    }
}
