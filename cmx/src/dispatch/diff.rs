use anyhow::Result;

use crate::context::AppContext;
use crate::types::ArtifactKind;

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
            runner.rt.block_on(crate::diff::diff_with_analysis(name, kind, full, &diff_ctx))
        }
        Err(e) => {
            let mut output = crate::diff::diff(name, kind, full, ctx)?;
            if !output.show_full && !output.is_up_to_date {
                output.analysis_note = Some(crate::diff::llm_unavailable_note(&e));
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
