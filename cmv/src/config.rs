//! Project configuration: the hand-authored `cmv.toml` at the project root
//! (see `CMV.md`, "Project config"). It holds what neither the atlas
//! nor the code can supply — an ecosystems override, the validator timeout,
//! and per-intent settings handed to validators as JSON. A missing file means
//! defaults.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use cmx_core::gateway::Filesystem;
use serde::Deserialize;
use serde_json::{Map, Value};

/// File name of the project configuration, at the project root.
pub const CONFIG_FILE_NAME: &str = "cmv.toml";

/// How long a validator may run before cmv kills it and reports the intent
/// unchecked, when `cmv.toml` does not say otherwise.
pub const DEFAULT_VALIDATOR_TIMEOUT_SECONDS: u64 = 60;

/// Parsed `cmv.toml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// Ecosystems to verify as, replacing the atlas's sensor detection
    /// entirely when present. cmv 3.2.x called this key `languages`; it still
    /// parses.
    #[serde(default, alias = "languages")]
    pub ecosystems: Option<Vec<String>>,
    /// Per-validator timeout; [`DEFAULT_VALIDATOR_TIMEOUT_SECONDS`] when absent.
    #[serde(default)]
    validator_timeout_seconds: Option<u64>,
    /// Per-intent settings, keyed by catalog key. Each table becomes the JSON
    /// object a validator receives through `--config`.
    #[serde(default)]
    pub intent: BTreeMap<String, toml::Table>,
}

impl ProjectConfig {
    /// How long each validator may run.
    pub fn validator_timeout(&self) -> Duration {
        Duration::from_secs(
            self.validator_timeout_seconds.unwrap_or(DEFAULT_VALIDATOR_TIMEOUT_SECONDS),
        )
    }

    /// The JSON object handed to validators of the intent `key`: its
    /// `[intent."<key>"]` table, or an empty object when the file has none.
    pub fn intent_config(&self, key: &str) -> Value {
        self.intent
            .get(key)
            .and_then(|table| serde_json::to_value(table).ok())
            .unwrap_or_else(|| Value::Object(Map::new()))
    }
}

/// Load `<root>/cmv.toml`, or defaults when it does not exist.
pub fn load(root: &Path, fs: &dyn Filesystem) -> Result<ProjectConfig> {
    let path = root.join(CONFIG_FILE_NAME);
    if !fs.is_file(&path) {
        return Ok(ProjectConfig::default());
    }
    let raw = fs.read_to_string(&path)?;
    toml::from_str(&raw).with_context(|| format!("could not parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::fakes::FakeFilesystem;
    use serde_json::json;

    const ROOT: &str = "/project";

    fn load_with(raw: &str) -> Result<ProjectConfig> {
        let fs = FakeFilesystem::new();
        fs.add_file("/project/cmv.toml", raw);
        load(Path::new(ROOT), &fs)
    }

    #[test]
    fn missing_file_yields_defaults() {
        let fs = FakeFilesystem::new();
        let config = load(Path::new(ROOT), &fs).unwrap();
        assert_eq!(config, ProjectConfig::default());
        assert_eq!(config.ecosystems, None);
        assert_eq!(config.validator_timeout(), Duration::from_secs(60));
        assert_eq!(config.intent_config("any/key"), json!({}));
    }

    #[test]
    fn parses_ecosystems_override_timeout_and_intent_tables() {
        let config = load_with(
            r#"
ecosystems = ["rust", "python"]
validator_timeout_seconds = 5

[intent."craftsperson/rust/isolate-functional-core"]
business_rule_pattern = "\\b500\\b"
business_rule_minimum_matches = 2
markers = ["a", "b"]

[intent."craftsperson/python/nonblocking-async-io".nested]
blocking_symbols = ["time.sleep"]
"#,
        )
        .unwrap();
        assert_eq!(
            config.ecosystems.as_deref(),
            Some(&["rust".to_string(), "python".to_string()][..])
        );
        assert_eq!(config.validator_timeout(), Duration::from_secs(5));
        assert_eq!(
            config.intent_config("craftsperson/rust/isolate-functional-core"),
            json!({
                "business_rule_pattern": "\\b500\\b",
                "business_rule_minimum_matches": 2,
                "markers": ["a", "b"],
            })
        );
        assert_eq!(
            config.intent_config("craftsperson/python/nonblocking-async-io"),
            json!({ "nested": { "blocking_symbols": ["time.sleep"] } })
        );
        assert_eq!(config.intent_config("unconfigured"), json!({}));
    }

    #[test]
    fn empty_file_is_all_defaults() {
        let config = load_with("").unwrap();
        assert_eq!(config, ProjectConfig::default());
    }

    #[test]
    fn unknown_top_level_key_is_rejected_with_the_path() {
        let error = load_with("ecosystem = [\"rust\"]\n").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("/project/cmv.toml"), "{message}");
        assert!(message.contains("ecosystem"), "{message}");
    }

    #[test]
    fn malformed_toml_is_rejected_with_the_path() {
        let error = load_with("ecosystems = [\n").unwrap_err();
        assert!(format!("{error:#}").contains("could not parse /project/cmv.toml"));
    }

    #[test]
    fn languages_key_from_cmv_3_2_still_parses_as_ecosystems() {
        let fs = FakeFilesystem::new();
        fs.add_file("/project/cmv.toml", "languages = [\"rust\"]\n");
        let config = load(Path::new("/project"), &fs).unwrap();
        assert_eq!(config.ecosystems.as_deref(), Some(&["rust".to_string()][..]));
    }
}
