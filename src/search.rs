use anyhow::Result;

use crate::config;
use crate::scan;
use crate::source;

struct SearchResult {
    name: String,
    kind: String,
    version: String,
    source: String,
    description: String,
}

pub fn search(query: &str) -> Result<()> {
    source::auto_update_all()?;

    let query_lower = query.to_lowercase();
    let sources = config::load_sources()?;
    let mut results = Vec::new();

    for (source_name, entry) in &sources.sources {
        let local_path = config::resolve_local_path(entry);
        if !local_path.exists() {
            continue;
        }
        if let Ok(artifacts) = scan::scan_source(&local_path) {
            for artifact in artifacts {
                let name_lower = artifact.name.to_lowercase();
                let desc_lower = artifact.description.to_lowercase();

                if name_lower.contains(&query_lower) || desc_lower.contains(&query_lower) {
                    // Truncate description to first meaningful chunk
                    let short_desc = truncate_description(&artifact.description, 80);

                    results.push(SearchResult {
                        name: artifact.name,
                        kind: artifact.kind.to_string(),
                        version: artifact.version.as_deref().unwrap_or("-").to_string(),
                        source: source_name.clone(),
                        description: short_desc,
                    });
                }
            }
        }
    }

    if results.is_empty() {
        println!("No results for '{query}'.");
        return Ok(());
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

    for r in &results {
        println!(
            "  {:<w_name$}  {:<w_kind$}  {:<w_ver$}  {:<w_src$}  {}",
            r.name, r.kind, r.version, r.source, r.description,
        );
    }

    println!();
    println!("{} result(s) found.", results.len());

    Ok(())
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
