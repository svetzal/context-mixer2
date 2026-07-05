use anyhow::Error;

const MAX_LEN: usize = 120;
const WRAPPER_PREFIXES: &[&str] = &["LLM analysis failed:", "LLM gateway error:"];

/// Reduce a nested gateway/provider error chain to a short, one-line phrase
/// suitable for CLI degradation notes.
pub fn summarize_gateway_error(error: &Error) -> String {
    let flattened = collapse_whitespace(&format!("{error:#}"));
    let without_wrappers = strip_wrappers(&flattened);
    let without_body =
        trim_suffix_at_any(without_wrappers, &[" - {", " - [", " caused by: ", " Caused by: "]);

    let summary = if let Some(openai) = provider_phrase(without_body, "OpenAI API error:") {
        openai
    } else if looks_like_ollama_unreachable(without_body) {
        "Ollama unreachable at localhost:11434".to_string()
    } else {
        without_body.to_string()
    };

    truncate(&summary)
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_wrappers(mut text: &str) -> &str {
    loop {
        let mut stripped = false;
        for prefix in WRAPPER_PREFIXES {
            if let Some(rest) = text.strip_prefix(prefix) {
                text = rest.trim_start();
                stripped = true;
                break;
            }
        }
        if !stripped {
            return text;
        }
    }
}

fn trim_suffix_at_any<'a>(text: &'a str, delimiters: &[&str]) -> &'a str {
    let cutoff = delimiters.iter().filter_map(|d| text.find(d)).min().unwrap_or(text.len());
    text[..cutoff].trim_end_matches([' ', ':'])
}

fn provider_phrase(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)?;
    Some(text[start..].trim().to_string())
}

fn looks_like_ollama_unreachable(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("localhost:11434") || lower.contains("ollama"))
        && (lower.contains("connection refused")
            || lower.contains("failed to connect")
            || lower.contains("error sending request")
            || lower.contains("tcp connect error"))
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_LEN {
        text.to_string()
    } else {
        let head: String = text.chars().take(MAX_LEN).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_gateway_error;

    #[test]
    fn summarize_gateway_error_keeps_short_message() {
        assert_eq!(summarize_gateway_error(&anyhow::anyhow!("short error")), "short error");
    }

    #[test]
    fn summarize_gateway_error_strips_openai_json_body() {
        let error = anyhow::anyhow!(
            "LLM analysis failed: LLM gateway error: OpenAI API error: 401 Unauthorized - {{ \
             \"error\": {{ \"message\": \"You didn't provide an API key\", \"type\": \
             \"invalid_request_error\" }} }}"
        );

        let summary = summarize_gateway_error(&error);
        assert_eq!(summary, "OpenAI API error: 401 Unauthorized");
        assert!(!summary.contains('{'));
        assert!(!summary.contains('\n'));
        assert!(!summary.contains("\"error\""));
    }
}
