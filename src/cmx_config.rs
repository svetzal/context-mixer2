use anyhow::{Result, bail};

use crate::config;
use crate::types::LlmGatewayType;

pub fn show() -> Result<()> {
    let cfg = config::load_config()?;
    println!("LLM gateway: {}", cfg.llm.gateway);
    println!("LLM model:   {}", cfg.llm.model);
    Ok(())
}

pub fn set_gateway(value: &str) -> Result<()> {
    let mut cfg = config::load_config()?;
    cfg.llm.gateway = match value {
        "openai" => LlmGatewayType::OpenAI,
        "ollama" => LlmGatewayType::Ollama,
        _ => bail!("Unknown gateway '{}'. Use 'openai' or 'ollama'.", value),
    };
    config::save_config(&cfg)?;
    println!("LLM gateway set to: {value}");
    Ok(())
}

pub fn set_model(value: &str) -> Result<()> {
    let mut cfg = config::load_config()?;
    cfg.llm.model = value.to_string();
    config::save_config(&cfg)?;
    println!("LLM model set to: {value}");
    Ok(())
}
