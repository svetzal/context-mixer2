use anyhow::{Result, bail};

use crate::config;
use crate::context::AppContext;
use crate::types::LlmGatewayType;

pub struct ConfigShowResult {
    pub gateway: String,
    pub model: String,
}

pub struct ConfigSetResult {
    pub field: &'static str,
    pub value: String,
}

pub fn show_with(ctx: &AppContext<'_>) -> Result<ConfigShowResult> {
    let cfg = config::load_config_with(ctx.fs, ctx.paths)?;
    Ok(ConfigShowResult {
        gateway: cfg.llm.gateway.to_string(),
        model: cfg.llm.model.clone(),
    })
}

pub fn set_gateway_with(value: &str, ctx: &AppContext<'_>) -> Result<ConfigSetResult> {
    let mut cfg = config::load_config_with(ctx.fs, ctx.paths)?;
    cfg.llm.gateway = match value {
        "openai" => LlmGatewayType::OpenAI,
        "ollama" => LlmGatewayType::Ollama,
        _ => bail!("Unknown gateway '{value}'. Use 'openai' or 'ollama'."),
    };
    config::save_config_with(&cfg, ctx.fs, ctx.paths)?;
    Ok(ConfigSetResult {
        field: "gateway",
        value: value.to_string(),
    })
}

pub fn set_model_with(value: &str, ctx: &AppContext<'_>) -> Result<ConfigSetResult> {
    let mut cfg = config::load_config_with(ctx.fs, ctx.paths)?;
    cfg.llm.model = value.to_string();
    config::save_config_with(&cfg, ctx.fs, ctx.paths)?;
    Ok(ConfigSetResult {
        field: "model",
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use crate::test_support::{make_ctx, test_paths};
    use chrono::Utc;

    fn make_test_ctx<'a>(
        fs: &'a FakeFilesystem,
        git: &'a FakeGitClient,
        clock: &'a FakeClock,
        paths: &'a crate::paths::ConfigPaths,
    ) -> AppContext<'a> {
        make_ctx(fs, git, clock, paths)
    }

    #[test]
    fn show_returns_defaults_when_no_config_file() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);

        let result = show_with(&ctx).unwrap();
        assert_eq!(result.gateway, "openai");
        assert!(!result.model.is_empty());
    }

    #[test]
    fn set_gateway_openai_persists_and_round_trips() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);

        let result = set_gateway_with("openai", &ctx).unwrap();
        assert_eq!(result.field, "gateway");
        assert_eq!(result.value, "openai");

        let shown = show_with(&ctx).unwrap();
        assert_eq!(shown.gateway, "openai");
    }

    #[test]
    fn set_gateway_ollama_persists_and_round_trips() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);

        set_gateway_with("ollama", &ctx).unwrap();

        let shown = show_with(&ctx).unwrap();
        assert_eq!(shown.gateway, "ollama");
    }

    #[test]
    fn set_gateway_unknown_returns_error() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);

        match set_gateway_with("unknown-gw", &ctx) {
            Err(e) => assert!(
                e.to_string().contains("Unknown gateway"),
                "expected 'Unknown gateway' in error, got: {e}"
            ),
            Ok(_) => panic!("expected an error for unknown gateway"),
        }
    }

    #[test]
    fn set_model_persists_and_round_trips() {
        let fs = FakeFilesystem::new();
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc::now());
        let paths = test_paths();
        let ctx = make_test_ctx(&fs, &git, &clock, &paths);

        let result = set_model_with("gpt-4", &ctx).unwrap();
        assert_eq!(result.field, "model");
        assert_eq!(result.value, "gpt-4");

        let shown = show_with(&ctx).unwrap();
        assert_eq!(shown.model, "gpt-4");
    }
}
