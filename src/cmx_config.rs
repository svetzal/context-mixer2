use anyhow::{Result, bail};

use crate::config;
use crate::context::AppContext;
use crate::types::LlmGatewayType;

pub fn show_with(ctx: &AppContext<'_>) -> Result<()> {
    let cfg = config::load_config_with(ctx.fs, ctx.paths)?;
    println!("LLM gateway: {}", cfg.llm.gateway);
    println!("LLM model:   {}", cfg.llm.model);
    Ok(())
}

pub fn set_gateway_with(value: &str, ctx: &AppContext<'_>) -> Result<()> {
    let mut cfg = config::load_config_with(ctx.fs, ctx.paths)?;
    cfg.llm.gateway = match value {
        "openai" => LlmGatewayType::OpenAI,
        "ollama" => LlmGatewayType::Ollama,
        _ => bail!("Unknown gateway '{value}'. Use 'openai' or 'ollama'."),
    };
    config::save_config_with(&cfg, ctx.fs, ctx.paths)?;
    println!("LLM gateway set to: {value}");
    Ok(())
}

pub fn set_model_with(value: &str, ctx: &AppContext<'_>) -> Result<()> {
    let mut cfg = config::load_config_with(ctx.fs, ctx.paths)?;
    cfg.llm.model = value.to_string();
    config::save_config_with(&cfg, ctx.fs, ctx.paths)?;
    println!("LLM model set to: {value}");
    Ok(())
}
