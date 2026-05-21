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

pub fn show(ctx: &AppContext<'_>) -> Result<ConfigShowResult> {
    let cfg = config::load_config(ctx.fs, ctx.paths)?;
    Ok(ConfigShowResult {
        gateway: cfg.llm.gateway.to_string(),
        model: cfg.llm.model.clone(),
    })
}

pub fn set_gateway(value: &str, ctx: &AppContext<'_>) -> Result<ConfigSetResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    cfg.llm.gateway = match value {
        "openai" => LlmGatewayType::OpenAI,
        "ollama" => LlmGatewayType::Ollama,
        _ => bail!("Unknown gateway '{value}'. Use 'openai' or 'ollama'."),
    };
    config::save_config(&cfg, ctx.fs, ctx.paths)?;
    Ok(ConfigSetResult {
        field: "gateway",
        value: value.to_string(),
    })
}

pub fn set_model(value: &str, ctx: &AppContext<'_>) -> Result<ConfigSetResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    cfg.llm.model = value.to_string();
    config::save_config(&cfg, ctx.fs, ctx.paths)?;
    Ok(ConfigSetResult {
        field: "model",
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestContext;

    // --- Display for ConfigShowResult ---

    #[test]
    fn config_show_result_display() {
        let result = ConfigShowResult {
            gateway: "ollama".to_string(),
            model: "llama3".to_string(),
        };
        let out = result.to_string();
        assert!(out.contains("LLM gateway: ollama"));
        assert!(out.contains("LLM model:   llama3"));
    }

    // --- Display for ConfigSetResult ---

    #[test]
    fn config_set_result_display_gateway() {
        let result = ConfigSetResult {
            field: "gateway",
            value: "ollama".to_string(),
        };
        assert_eq!(result.to_string(), "LLM gateway set to: ollama\n");
    }

    #[test]
    fn config_set_result_display_model() {
        let result = ConfigSetResult {
            field: "model",
            value: "gemma2".to_string(),
        };
        assert_eq!(result.to_string(), "LLM model set to: gemma2\n");
    }

    #[test]
    fn show_returns_defaults_when_no_config_file() {
        let t = TestContext::new();
        let ctx = t.ctx();

        let result = show(&ctx).unwrap();
        assert_eq!(result.gateway, "openai");
        assert!(!result.model.is_empty());
    }

    #[test]
    fn set_gateway_openai_persists_and_round_trips() {
        let t = TestContext::new();
        let ctx = t.ctx();

        let result = set_gateway("openai", &ctx).unwrap();
        assert_eq!(result.field, "gateway");
        assert_eq!(result.value, "openai");

        let shown = show(&ctx).unwrap();
        assert_eq!(shown.gateway, "openai");
    }

    #[test]
    fn set_gateway_ollama_persists_and_round_trips() {
        let t = TestContext::new();
        let ctx = t.ctx();

        set_gateway("ollama", &ctx).unwrap();

        let shown = show(&ctx).unwrap();
        assert_eq!(shown.gateway, "ollama");
    }

    #[test]
    fn set_gateway_unknown_returns_error() {
        let t = TestContext::new();
        let ctx = t.ctx();

        match set_gateway("unknown-gw", &ctx) {
            Err(e) => assert!(
                e.to_string().contains("Unknown gateway"),
                "expected 'Unknown gateway' in error, got: {e}"
            ),
            Ok(_) => panic!("expected an error for unknown gateway"),
        }
    }

    #[test]
    fn set_model_persists_and_round_trips() {
        let t = TestContext::new();
        let ctx = t.ctx();

        let result = set_model("gpt-4", &ctx).unwrap();
        assert_eq!(result.field, "model");
        assert_eq!(result.value, "gpt-4");

        let shown = show(&ctx).unwrap();
        assert_eq!(shown.model, "gpt-4");
    }
}
