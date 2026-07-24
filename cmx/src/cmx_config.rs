//! `cmx config` subcommands (show, set, external, platforms — the
//! managed-platform allowlist that scopes install/uninstall/doctor).

use crate::error::{CliError, Result};
use serde::Serialize;

use crate::config;
use crate::context::AppContext;
use crate::platform::Platform;
use crate::types::LlmGatewayType;

/// Snapshot of the current cmx configuration, as reported by `cmx config show`.
#[derive(Clone, Debug, Serialize)]
pub struct ConfigShowResult {
    /// Configured LLM gateway (e.g. "openai", "ollama").
    pub gateway: String,
    /// Configured LLM model name.
    pub model: String,
    /// Configured external rules (paths/names excluded from management).
    pub external: Vec<String>,
    /// Currently managed platform allowlist.
    pub platforms: Vec<Platform>,
}

/// Result of listing or mutating the managed `platforms` set.
pub struct PlatformsResult {
    /// Human-facing verb: "Added", "Removed", "Already managed", "Not managed",
    /// or "Managed platforms" for a plain listing.
    pub action: &'static str,
    /// The platform that was added/removed, or `None` for a plain listing.
    pub platform: Option<Platform>,
    /// The resulting (or current) managed set.
    pub platforms: Vec<Platform>,
}

/// Result of updating a single scalar config field (gateway or model).
pub struct ConfigSetResult {
    /// Name of the field that was updated (e.g. "gateway", "model").
    pub field: &'static str,
    /// The new value that was persisted.
    pub value: String,
}

/// Result of listing or mutating the `external` rules.
pub struct ExternalResult {
    /// Human-facing verb: "Added", "Removed", "Already present", "Not present",
    /// or "External rules" for a plain listing.
    pub action: &'static str,
    /// The entry that was added/removed, or `None` for a plain listing.
    pub entry: Option<String>,
    /// The resulting (or current) external list.
    pub external: Vec<String>,
}

/// Load the current cmx configuration for display by `cmx config show`.
pub fn show(ctx: &AppContext<'_>) -> Result<ConfigShowResult> {
    let cfg = config::load_config(ctx.fs, ctx.paths)?;
    Ok(ConfigShowResult {
        gateway: cfg.llm.gateway.to_string(),
        model: cfg.llm.model.clone(),
        external: cfg.external.clone(),
        platforms: cfg.platforms.clone(),
    })
}

/// List the managed platforms.
pub fn platforms_list(ctx: &AppContext<'_>) -> Result<PlatformsResult> {
    let cfg = config::load_config(ctx.fs, ctx.paths)?;
    Ok(PlatformsResult {
        action: "Managed platforms",
        platform: None,
        platforms: cfg.platforms,
    })
}

/// Add a platform to the managed set. Idempotent; keeps the list sorted/deduped.
pub fn platforms_add(platform: Platform, ctx: &AppContext<'_>) -> Result<PlatformsResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    let action = if cfg.platforms.contains(&platform) {
        "Already managed"
    } else {
        cfg.platforms.push(platform);
        cfg.platforms.sort_by_key(ToString::to_string);
        config::save_config(&cfg, ctx.fs, ctx.paths)?;
        "Added"
    };
    Ok(PlatformsResult {
        action,
        platform: Some(platform),
        platforms: cfg.platforms,
    })
}

/// Remove a platform from the managed set. Reports "Not managed" without error
/// if absent.
pub fn platforms_remove(platform: Platform, ctx: &AppContext<'_>) -> Result<PlatformsResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    let before = cfg.platforms.len();
    cfg.platforms.retain(|p| *p != platform);
    let action = if cfg.platforms.len() == before {
        "Not managed"
    } else {
        config::save_config(&cfg, ctx.fs, ctx.paths)?;
        "Removed"
    };
    Ok(PlatformsResult {
        action,
        platform: Some(platform),
        platforms: cfg.platforms,
    })
}

/// List the configured external rules.
pub fn external_list(ctx: &AppContext<'_>) -> Result<ExternalResult> {
    let cfg = config::load_config(ctx.fs, ctx.paths)?;
    Ok(ExternalResult {
        action: "External rules",
        entry: None,
        external: cfg.external,
    })
}

/// Add an external rule (a directory path or a bare artifact name). Idempotent.
pub fn external_add(entry: &str, ctx: &AppContext<'_>) -> Result<ExternalResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    let action = if cfg.external.iter().any(|e| e == entry) {
        "Already present"
    } else {
        cfg.external.push(entry.to_string());
        cfg.external.sort();
        config::save_config(&cfg, ctx.fs, ctx.paths)?;
        "Added"
    };
    Ok(ExternalResult {
        action,
        entry: Some(entry.to_string()),
        external: cfg.external,
    })
}

/// Remove an external rule. Reports "Not present" without error if absent.
pub fn external_remove(entry: &str, ctx: &AppContext<'_>) -> Result<ExternalResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    let before = cfg.external.len();
    cfg.external.retain(|e| e != entry);
    let action = if cfg.external.len() == before {
        "Not present"
    } else {
        config::save_config(&cfg, ctx.fs, ctx.paths)?;
        "Removed"
    };
    Ok(ExternalResult {
        action,
        entry: Some(entry.to_string()),
        external: cfg.external,
    })
}

/// Set the configured LLM gateway (`"openai"` or `"ollama"`). Returns
/// [`CliError::UnknownGateway`] for any other value.
pub fn set_gateway(value: &str, ctx: &AppContext<'_>) -> Result<ConfigSetResult> {
    let mut cfg = config::load_config(ctx.fs, ctx.paths)?;
    cfg.llm.gateway = match value {
        "openai" => LlmGatewayType::OpenAI,
        "ollama" => LlmGatewayType::Ollama,
        _ => {
            return Err(CliError::UnknownGateway {
                value: value.to_string(),
            });
        }
    };
    config::save_config(&cfg, ctx.fs, ctx.paths)?;
    Ok(ConfigSetResult {
        field: "gateway",
        value: value.to_string(),
    })
}

/// Set the configured LLM model name.
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
    fn external_add_list_remove_round_trip() {
        let t = TestContext::new();
        let ctx = t.ctx();

        // Empty to start.
        assert!(external_list(&ctx).unwrap().external.is_empty());

        // Add two rules.
        let added = external_add("~/.hermes/skills", &ctx).unwrap();
        assert_eq!(added.action, "Added");
        external_add("apple", &ctx).unwrap();

        let listed = external_list(&ctx).unwrap();
        assert!(listed.external.contains(&"~/.hermes/skills".to_string()));
        assert!(listed.external.contains(&"apple".to_string()));

        // Adding a duplicate is idempotent.
        assert_eq!(external_add("apple", &ctx).unwrap().action, "Already present");

        // Remove one; the other remains.
        assert_eq!(external_remove("apple", &ctx).unwrap().action, "Removed");
        let after = external_list(&ctx).unwrap();
        assert!(!after.external.contains(&"apple".to_string()));
        assert!(after.external.contains(&"~/.hermes/skills".to_string()));

        // Removing an absent rule reports Not present without error.
        assert_eq!(external_remove("apple", &ctx).unwrap().action, "Not present");
    }

    #[test]
    fn external_rules_surface_in_show() {
        let t = TestContext::new();
        let ctx = t.ctx();
        external_add("~/.hermes/skills", &ctx).unwrap();
        assert!(show(&ctx).unwrap().external.contains(&"~/.hermes/skills".to_string()));
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

    #[test]
    fn platforms_add_list_remove_roundtrip() {
        let t = TestContext::new();
        let ctx = t.ctx();

        assert_eq!(platforms_add(Platform::Codex, &ctx).unwrap().action, "Added");
        assert_eq!(platforms_add(Platform::Claude, &ctx).unwrap().action, "Added");
        assert_eq!(
            platforms_add(Platform::Codex, &ctx).unwrap().action,
            "Already managed",
            "adding a managed platform again is idempotent"
        );

        // Listed sorted by display name (claude before codex).
        assert_eq!(
            platforms_list(&ctx).unwrap().platforms,
            vec![Platform::Claude, Platform::Codex]
        );

        assert_eq!(platforms_remove(Platform::Claude, &ctx).unwrap().action, "Removed");
        assert_eq!(
            platforms_remove(Platform::Claude, &ctx).unwrap().action,
            "Not managed",
            "removing an unmanaged platform reports without erroring"
        );
        assert_eq!(platforms_list(&ctx).unwrap().platforms, vec![Platform::Codex]);

        // `show` surfaces the managed set too.
        assert_eq!(show(&ctx).unwrap().platforms, vec![Platform::Codex]);
    }
}
