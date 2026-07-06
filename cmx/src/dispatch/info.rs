use anyhow::Result;

use crate::context::AppContext;
use crate::types::ArtifactKind;

use super::print_json;

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
            Err(e) => info.summary_error = Some(crate::info::summary_unavailable_message(&e)),
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
