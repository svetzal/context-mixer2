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

#[cfg(test)]
mod tests {
    use super::analyze_focus;
    use crate::context::AppContext;
    use crate::diff::FocusedComparison;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient, FakeLlmClient};
    use crate::test_support::test_paths;
    use crate::types::ArtifactKind;
    use chrono::Utc;

    #[tokio::test]
    async fn analyze_focus_happy_path() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("analysis");

        let cmp = FocusedComparison {
            name: "my-agent",
            kind: ArtifactKind::Agent,
            source_name: "guidelines",
            changed_label: "codex",
            source_version: Some("1.0.0"),
            changed_version: Some("1.1.0"),
        };

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        analyze_focus(&cmp, "- old line\n+ new line\n", &ctx).await.unwrap();

        let (sys, usr) = llm.last_call().expect("analyze_focus must call the LLM");
        assert!(sys.contains("guidelines"), "system prompt names the source copy: {sys}");
        assert!(sys.contains("codex"), "system prompt names the changed copy: {sys}");
        assert!(
            sys.contains("belong to the 'guidelines' copy"),
            "- lines mapped to guidelines: {sys}"
        );
        assert!(sys.contains("belong to the 'codex' copy"), "+ lines mapped to codex: {sys}");
        assert!(usr.contains("my-agent"), "user prompt names the artifact: {usr}");
        assert!(usr.contains("agent"), "user prompt names the kind: {usr}");
        assert!(usr.contains("- old line"), "user prompt includes the diff: {usr}");
        assert!(usr.contains("1.0.0"), "user prompt includes source version: {usr}");
        assert!(usr.contains("1.1.0"), "user prompt includes changed version: {usr}");
    }

    #[tokio::test]
    async fn analyze_focus_unversioned_fallback() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let llm = FakeLlmClient::new("ok");

        let cmp = FocusedComparison {
            name: "my-agent",
            kind: ArtifactKind::Agent,
            source_name: "guidelines",
            changed_label: "codex",
            source_version: None,
            changed_version: None,
        };

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: Some(&llm),
        };
        analyze_focus(&cmp, "", &ctx).await.unwrap();

        let (_, usr) = llm.last_call().expect("analyze_focus must call the LLM");
        let count = usr.matches("unversioned").count();
        assert_eq!(count, 2, "both None versions must render as 'unversioned': {usr}");
    }

    #[tokio::test]
    async fn analyze_focus_without_llm_bails() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();

        let cmp = FocusedComparison {
            name: "my-agent",
            kind: ArtifactKind::Agent,
            source_name: "guidelines",
            changed_label: "codex",
            source_version: None,
            changed_version: None,
        };

        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: None,
        };
        let result = analyze_focus(&cmp, "", &ctx).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("LLM client not configured"),
            "error must mention 'LLM client not configured'"
        );
    }
}
