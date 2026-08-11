//! Test helpers for generating fake marketplace, plugin, and intent sources.

use std::path::PathBuf;

use cmx::gateway::fakes::FakeFilesystem;

use crate::repo::{RepoKind, RepoRoot};

/// Set up a fake marketplace repo and return its `RepoRoot`.
///
/// Writes `marketplace_json` to the `FakeFilesystem` under `/repo/.claude-plugin/marketplace.json`,
/// creates `/repo/plugins`, and returns a `RepoRoot` with `has_plugins_dir: true`.
pub fn fake_marketplace_root(fs: &FakeFilesystem, marketplace_json: &str) -> RepoRoot {
    fs.add_file("/repo/.claude-plugin/marketplace.json", marketplace_json);
    fs.add_dir("/repo/plugins");
    RepoRoot {
        path: PathBuf::from("/repo"),
        kind: RepoKind::Marketplace,
        has_intents: false,
        has_plugins_dir: true,
    }
}

/// Return a minimal marketplace `RepoRoot` for display tests that don't need filesystem state.
pub fn fake_marketplace_root_simple(path: &str) -> RepoRoot {
    RepoRoot {
        path: PathBuf::from(path),
        kind: RepoKind::Marketplace,
        has_intents: false,
        has_plugins_dir: false,
    }
}

/// Generate a valid `marketplace.json` string with the given plugin entries.
///
/// Each tuple is `(name, description, source_path)`.
pub fn fake_marketplace_json(plugins: &[(&str, &str, &str)]) -> String {
    let entries: Vec<String> = plugins
        .iter()
        .map(|(name, desc, source)| {
            format!(
                r#"    {{
      "name": "{name}",
      "description": "{desc}",
      "source": "{source}"
    }}"#
            )
        })
        .collect();

    format!(
        r#"{{
  "name": "svetzal-guidelines",
  "owner": {{
    "name": "Stacey Vetzal",
    "email": "stacey@vetzal.com"
  }},
  "plugins": [
{}
  ]
}}"#,
        entries.join(",\n")
    )
}

/// Generate a valid `marketplace.json` string with optional category per entry.
///
/// Each tuple is `(name, description, source_path, category)`.
pub fn fake_marketplace_json_with_categories(
    plugins: &[(&str, &str, &str, Option<&str>)],
) -> String {
    let entries: Vec<String> = plugins
        .iter()
        .map(|(name, desc, source, category)| {
            let cat_field = match category {
                Some(cat) => format!(",\n      \"category\": \"{cat}\""),
                None => String::new(),
            };
            format!(
                r#"    {{
      "name": "{name}",
      "description": "{desc}",
      "source": "{source}"{cat_field}
    }}"#
            )
        })
        .collect();

    format!(
        r#"{{
  "name": "svetzal-guidelines",
  "owner": {{
    "name": "Stacey Vetzal",
    "email": "stacey@vetzal.com"
  }},
  "plugins": [
{}
  ]
}}"#,
        entries.join(",\n")
    )
}

/// Generate a valid `plugin.json` string for the given plugin name.
pub fn fake_plugin_json(name: &str) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "description": "A plugin named {name}",
  "author": {{
    "name": "Test Author",
    "email": "test@example.com"
  }}
}}"#
    )
}

/// Generate a minimal valid intent record.
pub fn fake_intent_record(id: &str) -> String {
    format!(
        r#"id = "{id}"
title = "Verify the result"
category = "quality"
tags = ["verification"]
status = "hypothesized"
confidence = 0.99
capability = "Reliable completion"
threat = "Unchecked changes"
expectation = "Verification exposes defects"
strategy = "Run proportionate checks"
tradeoff = "Verification takes time"
evidence = [{{ type = "gate", description = "Checks pass", required = true }}]
scope = {{ project = "guidelines", paths = ["agents/test.md"] }}
sources = [{{ type = "document", ref = "agents/test.md", summary = "Requires checks", confidence = 1.0 }}]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::IntentRecord;
    use crate::plugin_types::{Marketplace, PluginManifest};

    #[test]
    fn fake_marketplace_json_is_valid() {
        let json = fake_marketplace_json(&[("test-plugin", "A test plugin", "./plugins/test")]);
        let mp: Marketplace = serde_json::from_str(&json).unwrap();
        assert_eq!(mp.name, "svetzal-guidelines");
        assert_eq!(mp.plugins.len(), 1);
        assert_eq!(mp.plugins[0].name, "test-plugin");
    }

    #[test]
    fn fake_plugin_json_is_valid() {
        let json = fake_plugin_json("my-plugin");
        let pm: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(pm.name, "my-plugin");
        assert_eq!(pm.version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn fake_intent_record_is_valid() {
        let source = fake_intent_record("guidelines.intent.verify");
        let intent: IntentRecord = toml::from_str(&source).unwrap();
        assert_eq!(intent.id, "guidelines.intent.verify");
        assert_eq!(intent.category, "quality");
    }
}
