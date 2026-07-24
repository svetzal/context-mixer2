//! `cmx info` command dispatch, a submodule of `cmx/src/dispatch/mod.rs`.

use anyhow::Result;

use crate::context::AppContext;
use crate::types::ArtifactKind;

use super::print_json;

/// Format a user-facing message when LLM summarization is unavailable.
///
/// Distinguishes gateway/auth failures (with actionable guidance) from other
/// errors (truncated raw message). Lives here because it takes `&anyhow::Error`
/// — the dispatch layer is the only code that should touch `anyhow`.
#[cfg(feature = "llm")]
pub(crate) fn summary_unavailable_message(e: &anyhow::Error) -> String {
    const MAX: usize = 200;

    if is_gateway_failure(e) {
        return format!(
            "summary unavailable — {}. Fix with 'cmx config gateway'/'cmx config model' or set OPENAI_API_KEY.",
            crate::error_summary::summarize_gateway_error(e)
        );
    }

    let flattened = format!("{e:#}").split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = if flattened.chars().count() > MAX {
        let head: String = flattened.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        flattened
    };
    if detail.ends_with(['.', '!', '?', '…']) {
        format!("summary unavailable — {detail}")
    } else {
        format!("summary unavailable — {detail}.")
    }
}

#[cfg(feature = "llm")]
fn is_gateway_failure(e: &anyhow::Error) -> bool {
    let rendered = format!("{e:#}");
    rendered.contains("LLM gateway error")
        || rendered.contains("OpenAI API error")
        || rendered.contains("localhost:11434")
        || rendered.contains("Ollama")
}

#[cfg(feature = "llm")]
use crate::gateway::real::MojenticLlmClient;

/// Bundles the LLM client and a current-thread tokio runtime, extracted from
/// the config at `ctx`. Both sites that need LLM access (`handle_info` and the
/// `Diff` action) build the same boilerplate; this helper captures it once.
#[cfg(feature = "llm")]
pub(crate) struct LlmRuntime {
    pub(crate) llm: MojenticLlmClient,
    pub(crate) rt: tokio::runtime::Runtime,
}

#[cfg(feature = "llm")]
pub(crate) fn build_llm_runtime(ctx: &AppContext<'_>) -> Result<LlmRuntime> {
    let cfg = crate::config::load_config(ctx.fs, ctx.paths)?;
    Ok(LlmRuntime {
        llm: MojenticLlmClient::new(cfg.llm),
        rt: tokio::runtime::Builder::new_current_thread().enable_all().build()?,
    })
}

/// Show details for an installed artifact. `kind` is `Some` for the kind-scoped
/// `cmx {skill,agent} info`, `None` for the top-level `cmx info` (searches both).
/// In an `llm`-feature build with a configured gateway it also attaches a
/// generated "what it does" summary, best-effort — a generation failure leaves
/// the summary blank rather than failing the command.
pub fn handle_info(
    name: &str,
    kind: Option<ArtifactKind>,
    json_output: bool,
    ctx: &AppContext<'_>,
) -> Result<()> {
    crate::source_update::ensure_fresh(ctx)?;
    #[cfg_attr(not(feature = "llm"), allow(unused_mut))]
    let mut info = match kind {
        Some(k) => crate::info::info_for_kind(name, k, ctx)?,
        None => crate::info::info(name, ctx)?,
    };

    #[cfg(feature = "llm")]
    {
        let runner = build_llm_runtime(ctx)?;
        let llm_ctx = ctx.with_llm(&runner.llm);
        match runner.rt.block_on(crate::info::summarize(&info, &llm_ctx)) {
            Ok(summary) => info.summary = Some(summary),
            // Best-effort: record *why* so the display reports the real reason
            // rather than always blaming the provider; never fail the command.
            Err(e) => {
                info.summary_error = Some(summary_unavailable_message(&anyhow::Error::from(e)));
            }
        }
    }

    if json_output {
        print_json(&crate::display::json::info_json(&info))?;
    } else {
        print!("{info}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::test_support::{fake_trio, make_test_ctx};

    #[test]
    fn handle_info_unknown_errors() {
        let (fs, git, clock, paths) = fake_trio();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);
        assert!(handle_info("nonexistent", None, false, &ctx).is_err());
    }
}
