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
