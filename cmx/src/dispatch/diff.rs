use anyhow::Result;

use crate::context::AppContext;
use crate::types::ArtifactKind;

/// One-line note shown in place of the analysis when the LLM summary failed
/// (gateway not configured, auth error, network error, ...). Never surfaces
/// the raw upstream error body.
#[cfg(feature = "llm")]
fn llm_unavailable_note(e: &anyhow::Error) -> String {
    format!(
        "note: LLM summary unavailable ({}). Fix the gateway (`cmx config gateway`, \
         `cmx config model`, or set OPENAI_API_KEY), or use --full for the plain diff.",
        crate::error_summary::summarize_gateway_error(e)
    )
}

/// `cmx {agent,skill} diff` — the structural diff always runs (no LLM
/// involved on `--full`); compact mode additionally attempts an LLM summary,
/// degrading to a one-line note on any failure (unconfigured gateway, auth
/// error, network error, or — in a lean build — no `llm` feature at all).
/// Only a genuine diff-compute error (artifact not found, unreadable files)
/// propagates as an `Err`.
#[cfg(feature = "llm")]
pub fn handle_diff(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<crate::diff::DiffOutput> {
    use crate::dispatch::info::build_llm_runtime;
    match build_llm_runtime(ctx) {
        Ok(runner) => {
            let diff_ctx = ctx.with_llm(&runner.llm);
            Ok(runner
                .rt
                .block_on(crate::diff::diff_with_analysis(name, kind, full, &diff_ctx))?)
        }
        Err(e) => {
            let mut output = crate::diff::diff(name, kind, full, ctx)?;
            if !output.show_full && !output.is_up_to_date {
                output.analysis_note = Some(llm_unavailable_note(&e));
            }
            Ok(output)
        }
    }
}

#[cfg(not(feature = "llm"))]
pub fn handle_diff(
    name: &str,
    kind: ArtifactKind,
    full: bool,
    ctx: &AppContext<'_>,
) -> Result<crate::diff::DiffOutput> {
    let mut output = crate::diff::diff(name, kind, full, ctx)?;
    if !output.show_full && !output.is_up_to_date {
        output.analysis_note = Some(crate::diff::llm_lean_note());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "llm")]
    #[test]
    fn llm_unavailable_note_strips_provider_json() {
        let error = anyhow::anyhow!(
            "LLM analysis failed: LLM gateway error: OpenAI API error: 401 Unauthorized - {{ \
             \"error\": {{ \"message\": \"missing key\" }} }}"
        );
        let note = super::llm_unavailable_note(&error);
        assert!(note.contains("OpenAI API error: 401 Unauthorized"), "{note}");
        assert!(note.contains("Fix the gateway"), "{note}");
        assert!(!note.contains('{'), "{note}");
        assert!(!note.contains("\"error\""), "{note}");
        assert!(!note.contains('\n'), "{note}");
    }
}
