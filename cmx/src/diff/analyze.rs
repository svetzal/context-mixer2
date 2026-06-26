use anyhow::{Result, bail};

use crate::context::AppContext;

use super::FocusedComparison;

/// Ask the LLM to summarize the focused copy's diff, naming the two sides by
/// their concrete identities (`source_name`, `changed`) so the summary speaks the
/// same language as the rest of the output (never "source"/"installed").
pub(super) async fn analyze_focus(
    cmp: &FocusedComparison<'_>,
    unified: &str,
    ctx: &AppContext<'_>,
) -> Result<String> {
    let source_ver = cmp.source_version.unwrap_or("unversioned");
    let changed_ver = cmp.changed_version.unwrap_or("unversioned");
    let system_prompt = format!(
        "You are a technical analyst comparing two copies of an AI coding assistant {kind} \
        (written in markdown). You are given a unified diff: lines prefixed with `-` belong to \
        the '{source_name}' copy; lines prefixed with `+` belong to the '{changed}' copy. \
        Refer to the two copies as '{source_name}' and '{changed}' — do not call them \
        \"source\" or \"installed\". Provide a clear, concise summary. Focus on:\n\
        1. What capabilities or behaviors were added, removed, or changed (and in which copy)\n\
        2. Whether the difference is significant or cosmetic\n\
        3. A recommendation: which copy looks more authoritative, and which way to reconcile\n\n\
        Keep it brief and actionable — a few paragraphs at most.",
        kind = cmp.kind,
        source_name = cmp.source_name,
        changed = cmp.changed_label,
    );
    let user_prompt = format!(
        "Compare these two copies of the {kind} '{name}':\n\
        - '{source_name}' copy (the `−` lines): {source_ver}\n\
        - '{changed}' copy (the `+` lines): {changed_ver}\n\n\
        {unified}",
        kind = cmp.kind,
        name = cmp.name,
        source_name = cmp.source_name,
        changed = cmp.changed_label,
    );
    match ctx.llm {
        Some(llm) => llm.analyze(&system_prompt, &user_prompt).await,
        None => bail!("LLM client not configured for diff analysis"),
    }
}
