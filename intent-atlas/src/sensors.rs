//! The atlas's **ecosystem sensors**: `ecosystems.toml` at the atlas root,
//! which declares every ecosystem the atlas supports and how to recognize it
//! in a project (see `CMV.md`, "Ecosystems and sensors").
//!
//! The atlas knows which ecosystems it supports; the tooling does not. The
//! names are the atlas's own vocabulary — each must be a directory the
//! realization hierarchy below `intents/` uses (`python`, `rust`, and `uv` for
//! `python/uv/`) — so the sensor file lives beside the records it describes,
//! is validated against the scanned catalog ([`validate`]), and is pinned
//! with everything else when cmv verifies at a recorded revision. Neither cmf
//! nor cmv carries a built-in table of build files: an atlas with no sensor
//! file detects nothing, and both binaries report that state rather than
//! guessing.
//!
//! Each ecosystem lists its `signatures`; it is **detected** when any one
//! holds, and its `implies` list is then detected too, transitively
//! ([`detect`]). The **predicate kinds** are the extension point:
//!
//! - `{ file = "<root-relative path>" }` — the path exists, as a file or a
//!   directory.
//! - `{ file = "<path>", contains = "<literal>" }` — the file exists and
//!   holds the literal.
//! - `{ glob = "<pattern>" }` — at least one root-level entry, file or
//!   directory, matches the pattern.
//!
//! A new kind — a dependency named in a manifest, a regular expression — is
//! one more [`Predicate`] variant and one more arm in the matcher, without
//! touching existing signatures. A predicate the tooling does not recognize
//! is a parse error naming the ecosystem and the signature, never a silent
//! miss. Every predicate is evaluated through the `Filesystem` gateway and
//! reads only the project root, so the matcher is pure, fixture-testable, and
//! deterministic.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use cmx_core::gateway::{DirEntry, Filesystem};
use glob::Pattern;
use serde::Deserialize;

use crate::catalog::{Intent, ecosystem_qualifiers};

/// File name of the sensor file, at the atlas root.
pub const SENSOR_FILE_NAME: &str = "ecosystems.toml";

/// One way to recognize an ecosystem at a project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// `{ file = "<path>" }`: the root-relative path exists, as a file or a
    /// directory.
    File {
        /// Root-relative path.
        path: String,
    },
    /// `{ file = "<path>", contains = "<literal>" }`: the root-relative file
    /// exists and holds the literal.
    FileContains {
        /// Root-relative path of the file to read.
        path: String,
        /// Substring the file must contain.
        literal: String,
    },
    /// `{ glob = "<pattern>" }`: at least one root-level entry, file or
    /// directory, has a name matching the pattern.
    Glob {
        /// The compiled pattern, matched against bare entry names.
        pattern: Pattern,
    },
}

/// One declared ecosystem: how to recognize it, and which ecosystems its
/// presence entails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ecosystem {
    /// Ecosystems detected whenever this one is; `uv` implies `python`.
    pub implies: Vec<String>,
    /// The ecosystem is detected when any of these holds. Never empty.
    pub signatures: Vec<Predicate>,
}

/// The parsed sensor file: every declared ecosystem, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sensors {
    ecosystems: BTreeMap<String, Ecosystem>,
}

/// The sensor file as serde reads it, before predicates are interpreted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEcosystem {
    #[serde(default)]
    implies: Vec<String>,
    signatures: Vec<toml::Table>,
}

impl Sensors {
    /// Parse the sensor file's text.
    ///
    /// Fails, naming the ecosystem and the signature, on an ecosystem with no
    /// signatures, a predicate with an unknown key or no recognized kind, a
    /// `contains` without `file`, a `glob` combined with `file`, an invalid
    /// glob, a non-string or empty value, or a `file` path that is absolute
    /// or climbs through `..` (a predicate reads only the project root).
    pub fn parse(raw: &str) -> Result<Self> {
        let declared: BTreeMap<String, RawEcosystem> = toml::from_str(raw)?;
        let mut ecosystems = BTreeMap::new();
        for (name, raw) in declared {
            if raw.signatures.is_empty() {
                bail!("ecosystem `{name}` declares no signatures");
            }
            let signatures = raw
                .signatures
                .iter()
                .enumerate()
                .map(|(index, table)| {
                    predicate(table).map_err(|reason| {
                        anyhow::anyhow!("ecosystem `{name}` signature {}: {reason}", index + 1)
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            ecosystems.insert(
                name,
                Ecosystem {
                    implies: raw.implies,
                    signatures,
                },
            );
        }
        Ok(Self { ecosystems })
    }

    /// Every declared ecosystem, in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Ecosystem)> {
        self.ecosystems.iter().map(|(name, ecosystem)| (name.as_str(), ecosystem))
    }

    /// The declared ecosystem `name`, if any.
    pub fn get(&self, name: &str) -> Option<&Ecosystem> {
        self.ecosystems.get(name)
    }

    /// How many ecosystems are declared.
    pub fn len(&self) -> usize {
        self.ecosystems.len()
    }

    /// Whether no ecosystem is declared; such a file detects nothing.
    pub fn is_empty(&self) -> bool {
        self.ecosystems.is_empty()
    }
}

/// Interpret one signature table as a predicate, or say why it is not one.
fn predicate(table: &toml::Table) -> std::result::Result<Predicate, String> {
    let mut file = None;
    let mut contains = None;
    let mut glob = None;
    for (key, value) in table {
        let Some(text) = value.as_str() else {
            return Err(format!("`{key}` must be a string"));
        };
        if text.is_empty() {
            return Err(format!("`{key}` must not be empty"));
        }
        match key.as_str() {
            "file" => file = Some(text),
            "contains" => contains = Some(text),
            "glob" => glob = Some(text),
            other => {
                return Err(format!(
                    "unknown key `{other}`; the predicate kinds are `file`, `file` with `contains`, and `glob`"
                ));
            }
        }
    }
    match (file, contains, glob) {
        (Some(path), contains, None) => {
            root_relative(path)?;
            Ok(match contains {
                Some(literal) => Predicate::FileContains {
                    path: path.to_string(),
                    literal: literal.to_string(),
                },
                None => Predicate::File {
                    path: path.to_string(),
                },
            })
        }
        (None, None, Some(pattern)) => Pattern::new(pattern)
            .map(|pattern| Predicate::Glob { pattern })
            .map_err(|error| format!("invalid glob {pattern:?}: {error}")),
        (None, Some(_), None) => {
            Err("`contains` needs `file`, the file that must hold the literal".to_string())
        }
        (None, None, None) => {
            Err("no predicate kind; give `file` (optionally with `contains`) or `glob`".to_string())
        }
        (_, _, Some(_)) => Err("`glob` cannot be combined with `file` or `contains`".to_string()),
    }
}

/// A `file` path resolves against the project root and must stay inside it.
fn root_relative(path: &str) -> std::result::Result<(), String> {
    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!("`file` path {path:?} must not contain `..`"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("`file` path {path:?} must be relative to the project root"));
            }
        }
    }
    Ok(())
}

/// Read `<root>/ecosystems.toml`, or `None` when the atlas has no sensor
/// file — a documented state (the atlas detects nothing), not an error.
pub fn load(root: &Path, fs: &dyn Filesystem) -> Result<Option<Sensors>> {
    let path = root.join(SENSOR_FILE_NAME);
    if !fs.is_file(&path) {
        return Ok(None);
    }
    let raw = fs.read_to_string(&path)?;
    Sensors::parse(&raw)
        .map(Some)
        .with_context(|| format!("could not parse sensors {}", path.display()))
}

/// Check the sensor file against the scanned catalog: every declared
/// ecosystem is a directory some record uses below `intents/`; every
/// `implies` target is a declared ecosystem; and wherever a record's
/// qualifiers nest (`…/python/uv/…`), a declared inner ecosystem (`uv`)
/// implies its parent (`python`). An ecosystem the catalog uses but the file
/// does not declare is simply undetectable, not an error.
pub fn validate(sensors: &Sensors, catalog: &BTreeMap<String, Intent>) -> Result<()> {
    let used: BTreeSet<&str> = catalog.keys().flat_map(|key| ecosystem_qualifiers(key)).collect();
    for name in sensors.ecosystems.keys() {
        if !used.contains(name.as_str()) {
            bail!(
                "sensor ecosystem `{name}` is not a directory any record uses below intents/; ecosystem names are the atlas's own realization hierarchy"
            );
        }
    }
    for (name, ecosystem) in &sensors.ecosystems {
        for target in &ecosystem.implies {
            if !sensors.ecosystems.contains_key(target) {
                bail!("ecosystem `{name}` implies `{target}`, which is not a declared ecosystem");
            }
        }
    }
    for key in catalog.keys() {
        for pair in ecosystem_qualifiers(key).windows(2) {
            let (parent, inner) = (pair[0], pair[1]);
            let implied = sensors
                .ecosystems
                .get(inner)
                .is_none_or(|ecosystem| ecosystem.implies.iter().any(|name| name == parent));
            if !implied {
                bail!(
                    "ecosystem `{inner}` must declare implies = [\"{parent}\"]: record {key} nests it below `{parent}`"
                );
            }
        }
    }
    Ok(())
}

/// [`load`] then [`validate`]: how cmf and cmv read the sensors of an atlas
/// whose catalog they have just scanned.
pub fn load_validated(
    root: &Path,
    catalog: &BTreeMap<String, Intent>,
    fs: &dyn Filesystem,
) -> Result<Option<Sensors>> {
    let sensors = load(root, fs)?;
    if let Some(sensors) = &sensors {
        validate(sensors, catalog).with_context(|| {
            format!("invalid sensors {}", root.join(SENSOR_FILE_NAME).display())
        })?;
    }
    Ok(sensors)
}

/// The ecosystems present at `root`: every declared ecosystem one of whose
/// signatures holds, plus everything those imply, transitively. Sorted and
/// deduplicated. Reads only the project root; a root that cannot be listed
/// matches no glob.
pub fn detect(sensors: &Sensors, root: &Path, fs: &dyn Filesystem) -> Vec<String> {
    let entries = fs.read_dir(root).unwrap_or_default();
    let mut detected: BTreeSet<String> = sensors
        .ecosystems
        .iter()
        .filter(|(_, ecosystem)| {
            ecosystem
                .signatures
                .iter()
                .any(|predicate| holds(predicate, root, &entries, fs))
        })
        .map(|(name, _)| name.clone())
        .collect();
    let mut pending: Vec<String> = detected.iter().cloned().collect();
    while let Some(name) = pending.pop() {
        let Some(ecosystem) = sensors.ecosystems.get(&name) else {
            continue;
        };
        for implied in &ecosystem.implies {
            if detected.insert(implied.clone()) {
                pending.push(implied.clone());
            }
        }
    }
    detected.into_iter().collect()
}

fn holds(predicate: &Predicate, root: &Path, entries: &[DirEntry], fs: &dyn Filesystem) -> bool {
    match predicate {
        Predicate::File { path } => fs.exists(&root.join(path)),
        Predicate::FileContains { path, literal } => {
            let path = root.join(path);
            fs.is_file(&path)
                && fs.read_to_string(&path).is_ok_and(|content| content.contains(literal))
        }
        Predicate::Glob { pattern } => {
            entries.iter().any(|entry| pattern.matches(&entry.file_name))
        }
    }
}

/// What detection found at a project root, keeping "the atlas cannot detect"
/// apart from "the atlas detected nothing": an atlas with no sensor file (or
/// one declaring no ecosystems) is [`Detection::NoSensors`], which the
/// tooling reports rather than treating as an empty workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// The atlas declares no sensors; nothing can be detected.
    NoSensors,
    /// The detected ecosystems, sorted and deduplicated; possibly empty.
    Detected(Vec<String>),
}

impl Detection {
    /// Run the atlas's sensors, when it has any, against `root`.
    pub fn sense(sensors: Option<&Sensors>, root: &Path, fs: &dyn Filesystem) -> Self {
        match sensors {
            Some(sensors) if !sensors.is_empty() => Self::Detected(detect(sensors, root, fs)),
            _ => Self::NoSensors,
        }
    }
}

/// The ecosystems in `declared` (a profile's `[select] ecosystems`, as the
/// manifest records them) that `detected` does not contain, in declared
/// order — the one comparison behind cmf's mismatch warning and cmv's
/// `profile_mismatch`.
pub fn undetected(declared: &[String], detected: &[String]) -> Vec<String> {
    declared.iter().filter(|name| !detected.contains(name)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::fakes::FakeFilesystem;

    const ROOT: &str = "/project";

    const PYTHON_AND_UV: &str = r#"
[python]
signatures = [
  { file = "pyproject.toml" },
  { file = "setup.py" },
  { glob = "requirements*.txt" },
]

[uv]
implies = ["python"]
signatures = [
  { file = "uv.lock" },
  { file = "pyproject.toml", contains = "[tool.uv]" },
]
"#;

    fn project(files: &[(&str, &str)], dirs: &[&str]) -> FakeFilesystem {
        let fs = FakeFilesystem::new();
        fs.add_dir(ROOT);
        for (name, content) in files {
            fs.add_file(format!("{ROOT}/{name}"), *content);
        }
        for dir in dirs {
            fs.add_dir(format!("{ROOT}/{dir}"));
        }
        fs
    }

    fn detect_in(raw: &str, fs: &FakeFilesystem) -> Vec<String> {
        detect(&Sensors::parse(raw).expect("sensors parse"), Path::new(ROOT), fs)
    }

    fn parse_error(raw: &str) -> String {
        format!("{:#}", Sensors::parse(raw).expect_err("sensor file is rejected"))
    }

    #[test]
    fn parses_every_predicate_kind_and_implies() {
        let sensors = Sensors::parse(PYTHON_AND_UV).unwrap();
        assert_eq!(sensors.len(), 2);
        let python = sensors.get("python").unwrap();
        assert!(python.implies.is_empty());
        assert_eq!(
            python.signatures,
            vec![
                Predicate::File {
                    path: "pyproject.toml".to_string()
                },
                Predicate::File {
                    path: "setup.py".to_string()
                },
                Predicate::Glob {
                    pattern: Pattern::new("requirements*.txt").unwrap()
                },
            ]
        );
        let uv = sensors.get("uv").unwrap();
        assert_eq!(uv.implies, ["python"]);
        assert_eq!(
            uv.signatures[1],
            Predicate::FileContains {
                path: "pyproject.toml".to_string(),
                literal: "[tool.uv]".to_string(),
            }
        );
        let names: Vec<&str> = sensors.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["python", "uv"]);
    }

    #[test]
    fn empty_file_declares_nothing() {
        let sensors = Sensors::parse("").unwrap();
        assert!(sensors.is_empty());
        assert_eq!(sensors, Sensors::default());
    }

    #[test]
    fn rejects_an_ecosystem_without_signatures() {
        assert_eq!(
            parse_error("[rust]\nsignatures = []\n"),
            "ecosystem `rust` declares no signatures"
        );
    }

    #[test]
    fn rejects_an_unknown_predicate_key_naming_the_signature() {
        let message = parse_error(
            "[rust]\nsignatures = [{ file = \"Cargo.toml\" }, { manifest = \"Cargo.toml\" }]\n",
        );
        assert_eq!(
            message,
            "ecosystem `rust` signature 2: unknown key `manifest`; the predicate kinds are `file`, `file` with `contains`, and `glob`"
        );
    }

    #[test]
    fn rejects_contains_without_file() {
        assert_eq!(
            parse_error("[uv]\nsignatures = [{ contains = \"[tool.uv]\" }]\n"),
            "ecosystem `uv` signature 1: `contains` needs `file`, the file that must hold the literal"
        );
    }

    #[test]
    fn rejects_a_signature_with_no_kind_and_a_glob_mixed_with_file() {
        assert_eq!(
            parse_error("[rust]\nsignatures = [{}]\n"),
            "ecosystem `rust` signature 1: no predicate kind; give `file` (optionally with `contains`) or `glob`"
        );
        assert_eq!(
            parse_error("[rust]\nsignatures = [{ file = \"Cargo.toml\", glob = \"*.rs\" }]\n"),
            "ecosystem `rust` signature 1: `glob` cannot be combined with `file` or `contains`"
        );
    }

    #[test]
    fn rejects_non_string_empty_and_invalid_values() {
        assert_eq!(
            parse_error("[rust]\nsignatures = [{ file = 1 }]\n"),
            "ecosystem `rust` signature 1: `file` must be a string"
        );
        assert_eq!(
            parse_error("[rust]\nsignatures = [{ glob = \"\" }]\n"),
            "ecosystem `rust` signature 1: `glob` must not be empty"
        );
        assert!(
            parse_error("[rust]\nsignatures = [{ glob = \"[\" }]\n")
                .starts_with("ecosystem `rust` signature 1: invalid glob \"[\": "),
        );
    }

    #[test]
    fn rejects_file_paths_that_leave_the_project_root() {
        assert_eq!(
            parse_error("[rust]\nsignatures = [{ file = \"../Cargo.toml\" }]\n"),
            "ecosystem `rust` signature 1: `file` path \"../Cargo.toml\" must not contain `..`"
        );
        assert_eq!(
            parse_error("[rust]\nsignatures = [{ file = \"/etc/passwd\" }]\n"),
            "ecosystem `rust` signature 1: `file` path \"/etc/passwd\" must be relative to the project root"
        );
    }

    #[test]
    fn rejects_unknown_ecosystem_fields() {
        let message = parse_error(
            "[rust]\nrequires = [\"cargo\"]\nsignatures = [{ file = \"Cargo.toml\" }]\n",
        );
        assert!(message.contains("requires"), "{message}");
    }

    #[test]
    fn load_returns_none_without_a_sensor_file_and_names_the_file_on_a_parse_error() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/kb");
        assert_eq!(load(Path::new("/kb"), &fs).unwrap(), None);

        fs.add_file("/kb/ecosystems.toml", PYTHON_AND_UV);
        assert_eq!(load(Path::new("/kb"), &fs).unwrap().unwrap().len(), 2);

        fs.add_file("/kb/ecosystems.toml", "[rust]\nsignatures = []\n");
        let message = format!("{:#}", load(Path::new("/kb"), &fs).unwrap_err());
        assert_eq!(
            message,
            "could not parse sensors /kb/ecosystems.toml: ecosystem `rust` declares no signatures"
        );
    }

    #[test]
    fn file_predicate_matches_a_file_or_a_directory_at_the_path() {
        let raw = "[rust]\nsignatures = [{ file = \"Cargo.toml\" }]\n[swift]\nsignatures = [{ file = \"App.xcodeproj\" }]\n";
        assert_eq!(detect_in(raw, &project(&[("Cargo.toml", "")], &[])), ["rust"]);
        assert_eq!(detect_in(raw, &project(&[], &["App.xcodeproj"])), ["swift"]);
        assert!(detect_in(raw, &project(&[("README.md", "")], &[])).is_empty());
    }

    #[test]
    fn file_predicate_may_look_below_the_root() {
        let raw = "[android]\nsignatures = [{ file = \"app/build.gradle.kts\" }]\n";
        assert_eq!(detect_in(raw, &project(&[("app/build.gradle.kts", "")], &[])), ["android"]);
    }

    #[test]
    fn contains_predicate_needs_the_literal_in_an_existing_readable_file() {
        let raw = "[phoenix]\nsignatures = [{ file = \"mix.exs\", contains = \":phoenix\" }]\n";
        assert_eq!(
            detect_in(raw, &project(&[("mix.exs", "deps: [{:phoenix, \"~> 1.7\"}]")], &[])),
            ["phoenix"]
        );
        assert!(detect_in(raw, &project(&[("mix.exs", "deps: []")], &[])).is_empty());
        assert!(detect_in(raw, &project(&[], &[])).is_empty(), "the file is absent");
        assert!(
            detect_in(raw, &project(&[], &["mix.exs"])).is_empty(),
            "a directory is not read"
        );
        let binary = project(&[], &[]);
        binary.add_file(format!("{ROOT}/mix.exs"), vec![0xff, 0xfe]);
        assert!(detect_in(raw, &binary).is_empty(), "an unreadable file does not hold anything");
    }

    #[test]
    fn glob_predicate_matches_root_level_entries_of_either_kind_only() {
        let raw = "[csharp]\nsignatures = [{ glob = \"*.sln\" }, { glob = \"*.csproj\" }]\n";
        assert_eq!(detect_in(raw, &project(&[("App.sln", "")], &[])), ["csharp"]);
        assert_eq!(detect_in(raw, &project(&[], &["App.csproj"])), ["csharp"]);
        assert!(detect_in(raw, &project(&[("src/App.csproj", "")], &[])).is_empty());
        assert!(detect_in(raw, &project(&[("App.sln.bak", "")], &[])).is_empty());
    }

    #[test]
    fn implies_are_added_transitively_and_the_result_is_sorted() {
        let raw = "[a]\nsignatures = [{ file = \"a\" }]\n[b]\nimplies = [\"a\"]\nsignatures = [{ file = \"b\" }]\n[c]\nimplies = [\"b\"]\nsignatures = [{ file = \"c\" }]\n";
        assert_eq!(detect_in(raw, &project(&[("c", "")], &[])), ["a", "b", "c"]);
        assert_eq!(detect_in(raw, &project(&[("b", "")], &[])), ["a", "b"]);
        assert_eq!(detect_in(raw, &project(&[("a", "")], &[])), ["a"]);
    }

    #[test]
    fn nothing_is_detected_in_an_empty_or_missing_root() {
        assert!(detect_in(PYTHON_AND_UV, &project(&[], &[])).is_empty());
        let sensors = Sensors::parse(PYTHON_AND_UV).unwrap();
        assert!(detect(&sensors, Path::new("/nowhere"), &FakeFilesystem::new()).is_empty());
    }

    #[test]
    fn python_and_uv_example_detects_as_documented() {
        assert_eq!(detect_in(PYTHON_AND_UV, &project(&[("setup.py", "")], &[])), ["python"]);
        assert_eq!(
            detect_in(PYTHON_AND_UV, &project(&[("requirements-dev.txt", "")], &[])),
            ["python"]
        );
        assert_eq!(
            detect_in(PYTHON_AND_UV, &project(&[("pyproject.toml", "[tool.uv]\n")], &[])),
            ["python", "uv"]
        );
        assert_eq!(
            detect_in(PYTHON_AND_UV, &project(&[("uv.lock", "")], &[])),
            ["python", "uv"],
            "uv alone implies python"
        );
    }

    #[test]
    fn detection_distinguishes_no_sensors_from_nothing_detected() {
        let fs = project(&[], &[]);
        assert_eq!(Detection::sense(None, Path::new(ROOT), &fs), Detection::NoSensors);
        assert_eq!(
            Detection::sense(Some(&Sensors::default()), Path::new(ROOT), &fs),
            Detection::NoSensors
        );
        let sensors = Sensors::parse(PYTHON_AND_UV).unwrap();
        assert_eq!(
            Detection::sense(Some(&sensors), Path::new(ROOT), &fs),
            Detection::Detected(vec![])
        );
    }

    #[test]
    fn undetected_keeps_declared_order() {
        let declared = ["rust".to_string(), "python".to_string(), "uv".to_string()];
        let detected = ["python".to_string()];
        assert_eq!(undetected(&declared, &detected), ["rust", "uv"]);
        assert!(undetected(&declared, &declared).is_empty());
        assert!(undetected(&[], &detected).is_empty());
    }

    // ---- validate -------------------------------------------------------

    const RECORD: &str = r#"
id = "kb.intent.x"
title = "X"
category = "quality"
status = "confirmed"
capability = "c"
threat = "t"
expectation = "e"
strategy = "s"
tradeoff = "o"
"#;

    fn catalog_with(keys: &[&str]) -> BTreeMap<String, Intent> {
        let fs = FakeFilesystem::new();
        fs.add_dir("/kb/intents");
        for key in keys {
            fs.add_file(format!("/kb/intents/{key}.toml"), RECORD);
        }
        crate::catalog::scan(Path::new("/kb"), &fs).expect("catalog scans")
    }

    fn validation_error(raw: &str, keys: &[&str]) -> String {
        let sensors = Sensors::parse(raw).unwrap();
        format!(
            "{:#}",
            validate(&sensors, &catalog_with(keys)).expect_err("sensors are rejected")
        )
    }

    #[test]
    fn accepts_sensors_whose_names_and_nesting_match_the_catalog() {
        let sensors = Sensors::parse(PYTHON_AND_UV).unwrap();
        let catalog = catalog_with(&[
            "craftsperson/python/type-boundaries",
            "craftsperson/python/uv/pin-interpreter",
            "craftsperson/general",
        ]);
        validate(&sensors, &catalog).unwrap();
        assert!(validate(&Sensors::default(), &catalog).is_ok(), "no sensors, nothing to check");
    }

    #[test]
    fn rejects_an_ecosystem_no_record_uses() {
        assert_eq!(
            validation_error(PYTHON_AND_UV, &["craftsperson/python/type-boundaries"]),
            "sensor ecosystem `uv` is not a directory any record uses below intents/; ecosystem names are the atlas's own realization hierarchy"
        );
    }

    #[test]
    fn rejects_an_implies_target_that_is_not_declared() {
        let raw = "[uv]\nimplies = [\"python\"]\nsignatures = [{ file = \"uv.lock\" }]\n";
        assert_eq!(
            validation_error(raw, &["craftsperson/python/uv/pin-interpreter"]),
            "ecosystem `uv` implies `python`, which is not a declared ecosystem"
        );
    }

    #[test]
    fn rejects_a_nested_ecosystem_that_does_not_imply_its_parent() {
        let raw = "[python]\nsignatures = [{ file = \"pyproject.toml\" }]\n[uv]\nsignatures = [{ file = \"uv.lock\" }]\n";
        assert_eq!(
            validation_error(raw, &["craftsperson/python/uv/pin-interpreter"]),
            "ecosystem `uv` must declare implies = [\"python\"]: record craftsperson/python/uv/pin-interpreter nests it below `python`"
        );
    }

    #[test]
    fn an_undeclared_nested_ecosystem_is_merely_undetectable() {
        let raw = "[python]\nsignatures = [{ file = \"pyproject.toml\" }]\n";
        let sensors = Sensors::parse(raw).unwrap();
        validate(&sensors, &catalog_with(&["craftsperson/python/uv/pin-interpreter"])).unwrap();
    }

    #[test]
    fn load_validated_names_the_file_on_a_validation_error() {
        let fs = FakeFilesystem::new();
        fs.add_file("/kb/ecosystems.toml", PYTHON_AND_UV);
        let catalog = catalog_with(&["craftsperson/python/type-boundaries"]);
        let message = format!("{:#}", load_validated(Path::new("/kb"), &catalog, &fs).unwrap_err());
        assert!(
            message.starts_with("invalid sensors /kb/ecosystems.toml: sensor ecosystem `uv`"),
            "{message}"
        );
        assert_eq!(load_validated(Path::new("/nokb"), &catalog, &fs).unwrap(), None);
    }
}
