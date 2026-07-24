//! Output formatting for `cmx config`, a submodule of
//! `cmx/src/display/mod.rs`.

use std::fmt;

use crate::cmx_config::{ConfigSetResult, ConfigShowResult, ExternalResult, PlatformsResult};
use crate::platform::platforms_label;

impl fmt::Display for ConfigShowResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LLM gateway: {}\nLLM model:   {}\n", self.gateway, self.model)?;
        if self.external.is_empty() {
            writeln!(f, "External:    (none)")?;
        } else {
            writeln!(f, "External:    {}", self.external.join(", "))?;
        }
        // "(inferred)" signals the fall-back behaviour: with no explicit set,
        // cmx infers which platforms to manage rather than using a fixed list.
        if self.platforms.is_empty() {
            writeln!(f, "Platforms:   (inferred)")
        } else {
            writeln!(f, "Platforms:   {}", platforms_label(&self.platforms))
        }
    }
}

impl fmt::Display for PlatformsResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(platform) = &self.platform {
            writeln!(f, "{} platform: {platform}", self.action)?;
        }
        if self.platforms.is_empty() {
            writeln!(f, "Managed platforms: (none — cmx infers from platforms in use)")
        } else {
            writeln!(f, "Managed platforms: {}", platforms_label(&self.platforms))
        }
    }
}

impl fmt::Display for ExternalResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(entry) = &self.entry {
            writeln!(f, "{} external rule: {entry}", self.action)?;
        }
        if self.external.is_empty() {
            writeln!(f, "External rules: (none)")
        } else {
            writeln!(f, "External rules:")?;
            for e in &self.external {
                writeln!(f, "  {e}")?;
            }
            Ok(())
        }
    }
}

impl fmt::Display for ConfigSetResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "LLM {} set to: {}", self.field, self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Step 12: ConfigShowResult and ConfigSetResult ---

    #[test]
    fn config_show_result_contains_gateway_and_model_labels() {
        let r = ConfigShowResult {
            gateway: "ollama".to_string(),
            model: "llama3".to_string(),
            external: vec!["~/.hermes/skills".to_string()],
            platforms: vec![crate::platform::Platform::Codex],
        };
        let out = r.to_string();
        assert!(out.contains("LLM gateway:"));
        assert!(out.contains("LLM model:"));
        assert!(out.contains("External:    ~/.hermes/skills"));
        assert!(out.contains("Platforms:   codex"));
    }

    #[test]
    fn external_result_list_and_mutation_render() {
        let list = ExternalResult {
            action: "External rules",
            entry: None,
            external: vec!["~/.hermes/skills".to_string()],
        };
        let out = list.to_string();
        assert!(out.contains("External rules:"));
        assert!(out.contains("~/.hermes/skills"));

        let added = ExternalResult {
            action: "Added",
            entry: Some("apple".to_string()),
            external: vec!["apple".to_string()],
        };
        assert!(added.to_string().contains("Added external rule: apple"));

        let empty = ExternalResult {
            action: "External rules",
            entry: None,
            external: vec![],
        };
        assert!(empty.to_string().contains("(none)"));
    }

    #[test]
    fn config_set_result_contains_field_and_value() {
        let r = ConfigSetResult {
            field: "model",
            value: "gpt-4".to_string(),
        };
        let out = r.to_string();
        assert!(out.contains("model"));
        assert!(out.contains("gpt-4"));
    }
}
