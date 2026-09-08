//! The ecosystems a run verifies as — which validators run — and where that
//! answer came from. An `ecosystems` override in `cmv.toml` replaces
//! detection entirely; otherwise the atlas's sensors
//! ([`intent_atlas::sensors`], read from the tree being verified) are
//! evaluated against the project root. An atlas that declares no sensors
//! cannot detect anything, and that is its own state rather than an empty
//! detection: every validator-bearing intent is reported unchecked with a
//! reason that names the override, and `status` and `explain` say the same.
//! Pure over the `Filesystem` gateway.

use std::path::Path;

use cmx_core::gateway::Filesystem;
use intent_atlas::catalog::{IntentRecord, Validator};
use intent_atlas::sensors::{Detection, Sensors};
use serde::Serialize;

/// The `unchecked` reason (and `explain`'s skip reason) when the atlas
/// declares no sensors and `cmv.toml` gives no override.
pub const NO_SENSORS: &str = "atlas declares no sensors; set ecosystems in cmv.toml to override";

/// The ecosystems the workspace verifies as. Serializes as the plain name
/// list — `[]` when nothing was detected or nothing could be — so the JSON
/// reports carry only `ecosystems`; the human listings render the state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "Vec<String>")]
pub enum Ecosystems {
    /// `cmv.toml` named them; detection did not run.
    Overridden(Vec<String>),
    /// The atlas's sensors found them at the project root.
    Detected(Vec<String>),
    /// The atlas declares no sensors and `cmv.toml` gives no override.
    Undetectable,
}

impl From<Ecosystems> for Vec<String> {
    fn from(ecosystems: Ecosystems) -> Self {
        ecosystems.names().to_vec()
    }
}

impl Ecosystems {
    /// The `cmv.toml` override when present (sorted and deduplicated), else
    /// what the atlas's sensors detect at `root`.
    pub fn resolve(
        override_ecosystems: Option<&[String]>,
        sensors: Option<&Sensors>,
        root: &Path,
        fs: &dyn Filesystem,
    ) -> Self {
        if let Some(names) = override_ecosystems {
            let mut names = names.to_vec();
            names.sort();
            names.dedup();
            return Self::Overridden(names);
        }
        match Detection::sense(sensors, root, fs) {
            Detection::NoSensors => Self::Undetectable,
            Detection::Detected(names) => Self::Detected(names),
        }
    }

    /// The names, sorted; empty when nothing was detected or nothing could be.
    pub fn names(&self) -> &[String] {
        match self {
            Self::Overridden(names) | Self::Detected(names) => names,
            Self::Undetectable => &[],
        }
    }

    /// Whether a validator's `language` is among the workspace's ecosystems,
    /// so `check` would run it.
    pub fn admits(&self, validator: &Validator<'_>) -> bool {
        self.names().iter().any(|name| name == validator.language)
    }

    /// Why a validator reading `language` would not run here.
    pub fn skip_reason(&self, language: &str) -> String {
        match self {
            Self::Undetectable => NO_SENSORS.to_string(),
            _ => format!(
                "language {language} is not among the workspace's ecosystems [{}]",
                self.names().join(", ")
            ),
        }
    }

    /// The `unchecked` reason for a record none of whose validators run: the
    /// no-sensors reason when the record does declare validators that could
    /// not be selected, else the plain absence of a matching validator.
    pub fn unchecked_reason(&self, record: &IntentRecord) -> String {
        if matches!(self, Self::Undetectable) && record.validators().next().is_some() {
            return NO_SENSORS.to_string();
        }
        format!("no validator for ecosystems [{}]", self.names().join(", "))
    }

    /// The human `Ecosystems:` line body.
    pub fn describe(&self) -> String {
        match self {
            Self::Overridden(names) if names.is_empty() => "none (cmv.toml override)".to_string(),
            Self::Overridden(names) => format!("{} (cmv.toml override)", names.join(", ")),
            Self::Detected(names) if names.is_empty() => "none detected".to_string(),
            Self::Detected(names) => names.join(", "),
            Self::Undetectable => format!("none ({NO_SENSORS})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::fakes::FakeFilesystem;

    const ROOT: &str = "/project";

    fn sensors() -> Sensors {
        Sensors::parse("[rust]\nsignatures = [{ file = \"Cargo.toml\" }]\n").unwrap()
    }

    fn workspace(files: &[&str]) -> FakeFilesystem {
        let fs = FakeFilesystem::new();
        fs.add_dir(ROOT);
        for file in files {
            fs.add_file(format!("{ROOT}/{file}"), "");
        }
        fs
    }

    fn record(evidence: &str) -> IntentRecord {
        toml::from_str(&format!(
            r#"
id = "kb.intent.x"
title = "X"
category = "quality"
status = "confirmed"
capability = "c"
threat = "t"
expectation = "e"
strategy = "s"
tradeoff = "o"
evidence = [{evidence}]
"#
        ))
        .unwrap()
    }

    const RUST_CHECK: &str = r#"{ type = "static-check", language = "rust", run = "checks/rust/x.py", description = "Checked." }"#;

    #[test]
    fn override_replaces_detection_entirely_and_is_normalized() {
        let fs = workspace(&["Cargo.toml"]);
        let names = vec!["python".to_string(), "go".to_string(), "python".to_string()];
        let ecosystems = Ecosystems::resolve(Some(&names), Some(&sensors()), Path::new(ROOT), &fs);
        assert_eq!(
            ecosystems,
            Ecosystems::Overridden(vec!["go".to_string(), "python".to_string()])
        );
        assert_eq!(
            Ecosystems::resolve(Some(&[]), Some(&sensors()), Path::new(ROOT), &fs),
            Ecosystems::Overridden(vec![]),
            "an empty override verifies nothing"
        );
        assert_eq!(
            Ecosystems::resolve(Some(&[]), None, Path::new(ROOT), &fs),
            Ecosystems::Overridden(vec![]),
            "an override works without sensors"
        );
    }

    #[test]
    fn sensors_detect_and_no_sensors_is_undetectable() {
        let fs = workspace(&["Cargo.toml"]);
        assert_eq!(
            Ecosystems::resolve(None, Some(&sensors()), Path::new(ROOT), &fs),
            Ecosystems::Detected(vec!["rust".to_string()])
        );
        assert_eq!(
            Ecosystems::resolve(None, Some(&sensors()), Path::new("/elsewhere"), &fs),
            Ecosystems::Detected(vec![])
        );
        assert_eq!(Ecosystems::resolve(None, None, Path::new(ROOT), &fs), Ecosystems::Undetectable);
        assert_eq!(
            Ecosystems::resolve(None, Some(&Sensors::default()), Path::new(ROOT), &fs),
            Ecosystems::Undetectable,
            "a sensor file declaring nothing is no sensors"
        );
    }

    #[test]
    fn admits_validators_by_language() {
        let record = record(RUST_CHECK);
        let validator = record.validators().next().unwrap();
        assert!(Ecosystems::Detected(vec!["rust".to_string()]).admits(&validator));
        assert!(!Ecosystems::Detected(vec!["python".to_string()]).admits(&validator));
        assert!(!Ecosystems::Undetectable.admits(&validator));
    }

    #[test]
    fn reasons_name_the_state() {
        let detected = Ecosystems::Detected(vec!["rust".to_string()]);
        assert_eq!(
            detected.skip_reason("python"),
            "language python is not among the workspace's ecosystems [rust]"
        );
        assert_eq!(Ecosystems::Undetectable.skip_reason("python"), NO_SENSORS);
        assert_eq!(
            detected.unchecked_reason(&record(RUST_CHECK)),
            "no validator for ecosystems [rust]"
        );
        assert_eq!(Ecosystems::Undetectable.unchecked_reason(&record(RUST_CHECK)), NO_SENSORS);
        assert_eq!(
            Ecosystems::Undetectable.unchecked_reason(&record("")),
            "no validator for ecosystems []",
            "a record with no validator is unchecked for that reason, sensors or not"
        );
    }

    #[test]
    fn describes_every_state_for_the_human_line() {
        assert_eq!(Ecosystems::Detected(vec!["rust".to_string()]).describe(), "rust");
        assert_eq!(Ecosystems::Detected(vec![]).describe(), "none detected");
        assert_eq!(
            Ecosystems::Overridden(vec!["rust".to_string()]).describe(),
            "rust (cmv.toml override)"
        );
        assert_eq!(Ecosystems::Overridden(vec![]).describe(), "none (cmv.toml override)");
        assert_eq!(
            Ecosystems::Undetectable.describe(),
            "none (atlas declares no sensors; set ecosystems in cmv.toml to override)"
        );
    }

    #[test]
    fn serializes_as_the_plain_name_list() {
        assert_eq!(
            serde_json::to_string(&Ecosystems::Overridden(vec!["rust".to_string()])).unwrap(),
            "[\"rust\"]"
        );
        assert_eq!(serde_json::to_string(&Ecosystems::Undetectable).unwrap(), "[]");
    }
}
