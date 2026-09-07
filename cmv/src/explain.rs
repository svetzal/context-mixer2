//! `cmv explain <intent>`: what `check` would do for one compiled intent,
//! without running anything. Answers how the manifest entry resolves to a
//! record (by `key`, by a moved record's unique `id`, or not at all — naming
//! the records that share the id when that blocked the fallback), what the
//! record says, every
//! validator it declares, which of those would run for the workspace's
//! languages and with exactly which argv, the `--config` document they would
//! receive from `cmv.toml`, whether the intent is stale, and whether the
//! manifest dropped it. Pure over the `Filesystem` gateway; shares its
//! resolution, stale, and argv decisions with [`crate::dispatch`] so what it
//! prints is what `check` does.

use std::path::Path;

use anyhow::{Result, bail};
use cmx_core::gateway::Filesystem;
use intent_atlas::catalog::{Intent, Validator};
use intent_atlas::manifest::{DroppedIntent, IntentRef, Manifest};
use serde::Serialize;
use serde_json::Value;

use crate::config::ProjectConfig;
use crate::dispatch::{
    Catalog, RecordResolution, Resolution, Resolver, Trees, canonical_root, is_stale,
    language_matches, validator_args,
};
use crate::pin::AtlasReport;

/// Placeholder for the per-run scratch directory in a printed argv; the real
/// directory is created at check time and never appears in output.
pub const SCRATCH_PLACEHOLDER: &str = "<scratch>";

/// Placeholder for a materialized pinned tree in a printed argv, for the same
/// reason.
pub const PINNED_TREE_PLACEHOLDER: &str = "<pinned-tree>";

/// Everything [`explain`] needs besides the filesystem.
pub struct ExplainRequest<'a> {
    /// The manifest cmf compiled for the workspace.
    pub manifest: &'a Manifest,
    /// The atlas as scanned from `trees.verified`.
    pub catalog: &'a Catalog,
    /// The project's `cmv.toml`.
    pub config: &'a ProjectConfig,
    /// Languages the workspace verifies as.
    pub languages: &'a [String],
    /// Where the atlas was found and which tree was used.
    pub atlas: &'a AtlasReport,
    /// The atlas trees.
    pub trees: Trees<'a>,
    /// The project root validators would receive as `--workspace`.
    pub workspace: &'a Path,
}

/// The full `cmv explain` result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExplainReport {
    /// Report schema version.
    pub schema: u32,
    /// Where the records came from and which tree was used.
    pub atlas: AtlasReport,
    /// Languages the workspace verifies as.
    pub languages: Vec<String>,
    /// The explained intent.
    pub intent: IntentExplanation,
}

/// One intent, as `check` would treat it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntentExplanation {
    /// The manifest's catalog key.
    pub key: String,
    /// The record's `id`: from the manifest when compiled, else from the
    /// record when a dropped intent's record is still in the atlas.
    pub id: Option<String>,
    /// How the manifest entry was matched to a record.
    pub resolution: RecordResolution,
    /// Why the record was not found, when the manifest key is gone and its
    /// id is shared by several records — names them; `None` otherwise.
    pub resolution_detail: Option<String>,
    /// The record's title, when found.
    pub title: Option<String>,
    /// The record's maturity status, when found. Informational: it never
    /// changes the exit code.
    pub status: Option<String>,
    /// Whether the intent is in the manifest's `intents` (retained in the
    /// artifact).
    pub compiled: bool,
    /// Whether the intent is in the manifest's `dropped`.
    pub dropped: bool,
    /// The manifest's drop reason, when dropped.
    pub drop_reason: Option<String>,
    /// Whether the record or a validator changed at `HEAD` since compile.
    /// Always `false` for an intent that was only dropped: there is no
    /// compile-time checksum to compare.
    pub stale: bool,
    /// The JSON document validators would receive through `--config`.
    pub config: Value,
    /// Every validator the record declares, in declaration order; empty when
    /// the record was not found.
    pub validators: Vec<ValidatorPlan>,
}

/// One declared validator and whether `check` would run it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidatorPlan {
    /// Source language the validator reads.
    pub language: String,
    /// Executable path relative to the atlas root.
    pub run: String,
    /// Whether a failing verdict gates the run.
    pub required: bool,
    /// What it checks, as rendered into the guidance.
    pub description: String,
    /// Whether `check` would run it here.
    pub would_run: bool,
    /// Why not, when it would not.
    pub skipped: Option<String>,
    /// The argv `check` would use, with placeholders for the per-run scratch
    /// directory and a materialized pinned tree; `None` when the intent was
    /// not compiled, because no run would occur.
    pub argv: Option<Vec<String>>,
}

/// Explain `argument`: a catalog key, or a record `id` when it contains a
/// `.`. Fails when the manifest neither compiled nor dropped the intent.
pub fn explain(
    argument: &str,
    request: &ExplainRequest<'_>,
    fs: &dyn Filesystem,
) -> Result<ExplainReport> {
    let resolver = Resolver::new(request.catalog);
    let by_id = argument.contains('.');
    let compiled = request.manifest.intents.iter().enumerate().find(|(_, entry)| {
        if by_id {
            entry.id == argument
        } else {
            entry.key == argument
        }
    });
    let key: Option<&str> = match compiled {
        Some((_, entry)) => Some(&entry.key),
        None if by_id => resolver.by_id(argument).map(|intent| intent.key.as_str()),
        None => Some(argument),
    };
    let dropped = key.and_then(|key| request.manifest.dropped.iter().find(|d| d.key == key));
    let intent = match (compiled, dropped) {
        (Some((index, entry)), dropped) => {
            explain_compiled(index, entry, dropped, &resolver, request, fs)?
        }
        (None, Some(dropped)) => explain_dropped_only(dropped, &resolver, request),
        (None, None) => bail!(
            "intent {argument:?} is not in the manifest, neither compiled nor dropped; `cmv status` counts what was compiled, and the manifest lists the keys"
        ),
    };
    Ok(ExplainReport {
        schema: crate::report::SCHEMA_VERSION,
        atlas: request.atlas.clone(),
        languages: request.languages.to_vec(),
        intent,
    })
}

fn explain_compiled(
    index: usize,
    entry: &IntentRef,
    dropped: Option<&DroppedIntent>,
    resolver: &Resolver<'_>,
    request: &ExplainRequest<'_>,
    fs: &dyn Filesystem,
) -> Result<IntentExplanation> {
    let (record, resolution, resolution_detail) = match resolver.resolve(entry) {
        Resolution::Found { intent, how } => (Some(intent), how, None),
        Resolution::NotFound { detail } => (None, RecordResolution::NotFound, detail),
    };
    let stale = match record {
        Some(record) => is_stale(record, entry, request.trees, fs)?,
        None => false,
    };
    let validators = match record {
        Some(record) => {
            let root = canonical_root(request.trees.verified, fs)?;
            let program_root = match request.trees.working {
                Some(_) => Path::new(PINNED_TREE_PLACEHOLDER),
                None => root.as_path(),
            };
            let config_path = Path::new(SCRATCH_PLACEHOLDER).join(format!("{index}.json"));
            record
                .record
                .validators()
                .map(|validator| plan(&validator, request, program_root, &config_path))
                .collect()
        }
        None => vec![],
    };
    Ok(IntentExplanation {
        key: entry.key.clone(),
        id: Some(entry.id.clone()),
        resolution,
        resolution_detail,
        title: record.map(|record| record.record.title.clone()),
        status: record.map(|record| record.record.status.clone()),
        compiled: true,
        dropped: dropped.is_some(),
        drop_reason: dropped.map(|dropped| dropped.reason.clone()),
        stale,
        config: request.config.intent_config(&entry.key),
        validators,
    })
}

fn explain_dropped_only(
    dropped: &DroppedIntent,
    resolver: &Resolver<'_>,
    request: &ExplainRequest<'_>,
) -> IntentExplanation {
    let record: Option<&Intent> = resolver.by_key(&dropped.key);
    let validators = record
        .map(|record| {
            record
                .record
                .validators()
                .map(|validator| ValidatorPlan {
                    language: validator.language.to_string(),
                    run: validator.run.display().to_string(),
                    required: validator.required,
                    description: validator.description.to_string(),
                    would_run: false,
                    skipped: Some(format!(
                        "intent was dropped ({}); the guidance never reached the artifact",
                        dropped.reason
                    )),
                    argv: None,
                })
                .collect()
        })
        .unwrap_or_default();
    IntentExplanation {
        key: dropped.key.clone(),
        id: record.map(|record| record.record.id.clone()),
        resolution: if record.is_some() {
            RecordResolution::Key
        } else {
            RecordResolution::NotFound
        },
        resolution_detail: None,
        title: record.map(|record| record.record.title.clone()),
        status: record.map(|record| record.record.status.clone()),
        compiled: false,
        dropped: true,
        drop_reason: Some(dropped.reason.clone()),
        stale: false,
        config: request.config.intent_config(&dropped.key),
        validators,
    }
}

fn plan(
    validator: &Validator<'_>,
    request: &ExplainRequest<'_>,
    program_root: &Path,
    config_path: &Path,
) -> ValidatorPlan {
    let would_run = language_matches(validator, request.languages);
    let skipped = (!would_run).then(|| {
        format!(
            "language {} is not among the workspace's [{}]",
            validator.language,
            request.languages.join(", ")
        )
    });
    let mut argv = vec![program_root.join(validator.run).display().to_string()];
    argv.extend(
        validator_args(request.workspace, config_path)
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned()),
    );
    ValidatorPlan {
        language: validator.language.to_string(),
        run: validator.run.display().to_string(),
        required: validator.required,
        description: validator.description.to_string(),
        would_run,
        skipped,
        argv: Some(argv),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pin::VerifiedAgainst;
    use crate::resolve::ResolvedBy;
    use cmx_core::checksum;
    use cmx_core::gateway::fakes::FakeFilesystem;
    use intent_atlas::manifest::{ArtifactRef, Atlas, ProfileRef};
    use intent_atlas::profile::Surface;
    use serde_json::json;
    use std::path::PathBuf;

    const KB: &str = "/kb";
    const PINNED: &str = "/scratch/kb";
    const WORKSPACE: &str = "/project";

    const RECORD: &str = r#"
id = "kb.intent.isolate"
title = "Isolate the functional core"
category = "architecture"
status = "confirmed"
capability = "c"
threat = "t"
expectation = "e"
strategy = "s"
tradeoff = "o"
evidence = [
  { type = "architecture_review", description = "Reviewed.", required = true },
  { type = "static-check", language = "rust", run = "checks/rust/isolate.sh", description = "No rules beside I/O.", required = true },
  { type = "static-check", language = "python", run = "checks/python/isolate.py", description = "No rules beside clients.", required = false },
]
"#;

    struct Fixture {
        fs: FakeFilesystem,
        manifest: Manifest,
        config: ProjectConfig,
        atlas: AtlasReport,
    }

    impl Fixture {
        fn new() -> Self {
            let fs = FakeFilesystem::new();
            fs.add_dir(format!("{KB}/intents"));
            fs.add_file(format!("{KB}/intents/rust/isolate.toml"), RECORD);
            fs.add_file(format!("{KB}/checks/rust/isolate.sh"), "script");
            let manifest = Manifest {
                schema: 1,
                compiled_at: "2026-09-05T14:02:11+00:00".to_string(),
                atlas: Atlas {
                    source: None,
                    path: PathBuf::from(KB),
                    revision: None,
                },
                profile: ProfileRef {
                    id: "shipping".to_string(),
                    version: "0.1.0".to_string(),
                    ecosystems: vec![],
                },
                artifact: ArtifactRef {
                    name: "AGENTS".to_string(),
                    surface: Surface::Agent,
                    checksum: "sha256:artifact".to_string(),
                },
                intents: vec![
                    IntentRef {
                        id: "kb.intent.other".to_string(),
                        key: "rust/other".to_string(),
                        checksum: "sha256:0000".to_string(),
                    },
                    IntentRef {
                        id: "kb.intent.isolate".to_string(),
                        key: "rust/isolate".to_string(),
                        checksum: checksum::checksum_bytes(RECORD.as_bytes()),
                    },
                ],
                dropped: vec![DroppedIntent {
                    key: "rust/budgeted".to_string(),
                    reason: "budget".to_string(),
                }],
            };
            let config: ProjectConfig =
                toml::from_str("[intent.\"rust/isolate\"]\nmarkers = [\"x\"]\n").unwrap();
            Self {
                fs,
                manifest,
                config,
                atlas: AtlasReport {
                    path: PathBuf::from(KB),
                    resolved_by: ResolvedBy::Path,
                    source: None,
                    pinned_revision: None,
                    head_revision: None,
                    verified_against: VerifiedAgainst::Head,
                    moved: false,
                },
            }
        }

        fn explain(&self, argument: &str, languages: &[&str]) -> Result<ExplainReport> {
            self.explain_with(argument, languages, Trees::single(Path::new(KB)), Path::new(KB))
        }

        fn explain_with(
            &self,
            argument: &str,
            languages: &[&str],
            trees: Trees<'_>,
            catalog_root: &Path,
        ) -> Result<ExplainReport> {
            let catalog =
                intent_atlas::catalog::scan(catalog_root, &self.fs).expect("catalog scans");
            let languages: Vec<String> = languages.iter().map(ToString::to_string).collect();
            let request = ExplainRequest {
                manifest: &self.manifest,
                catalog: &catalog,
                config: &self.config,
                languages: &languages,
                atlas: &self.atlas,
                trees,
                workspace: Path::new(WORKSPACE),
            };
            explain(argument, &request, &self.fs)
        }
    }

    #[test]
    fn compiled_intent_by_key_lists_validators_with_argv_and_config() {
        let fixture = Fixture::new();
        let report = fixture.explain("rust/isolate", &["rust"]).unwrap();
        assert_eq!(report.schema, 1);
        assert_eq!(report.languages, ["rust"]);
        assert_eq!(report.atlas, fixture.atlas);
        let intent = report.intent;
        assert_eq!(intent.key, "rust/isolate");
        assert_eq!(intent.id.as_deref(), Some("kb.intent.isolate"));
        assert_eq!(intent.resolution, RecordResolution::Key);
        assert_eq!(intent.resolution_detail, None);
        assert_eq!(intent.title.as_deref(), Some("Isolate the functional core"));
        assert_eq!(intent.status.as_deref(), Some("confirmed"));
        assert!(intent.compiled);
        assert!(!intent.dropped);
        assert_eq!(intent.drop_reason, None);
        assert!(!intent.stale);
        assert_eq!(intent.config, json!({ "markers": ["x"] }));
        assert_eq!(
            intent.validators,
            vec![
                ValidatorPlan {
                    language: "rust".to_string(),
                    run: "checks/rust/isolate.sh".to_string(),
                    required: true,
                    description: "No rules beside I/O.".to_string(),
                    would_run: true,
                    skipped: None,
                    argv: Some(vec![
                        "/kb/checks/rust/isolate.sh".to_string(),
                        "--workspace".to_string(),
                        "/project".to_string(),
                        "--config".to_string(),
                        "<scratch>/1.json".to_string(),
                    ]),
                },
                ValidatorPlan {
                    language: "python".to_string(),
                    run: "checks/python/isolate.py".to_string(),
                    required: false,
                    description: "No rules beside clients.".to_string(),
                    would_run: false,
                    skipped: Some(
                        "language python is not among the workspace's [rust]".to_string()
                    ),
                    argv: Some(vec![
                        "/kb/checks/python/isolate.py".to_string(),
                        "--workspace".to_string(),
                        "/project".to_string(),
                        "--config".to_string(),
                        "<scratch>/1.json".to_string(),
                    ]),
                },
            ]
        );
    }

    #[test]
    fn an_argument_with_a_dot_is_matched_by_id() {
        let fixture = Fixture::new();
        let report = fixture.explain("kb.intent.isolate", &["rust"]).unwrap();
        assert_eq!(report.intent.key, "rust/isolate");
        assert_eq!(report.intent.resolution, RecordResolution::Key);
    }

    #[test]
    fn a_shared_id_argument_is_not_matched_through_the_catalog() {
        let fixture = Fixture::new();
        fixture.fs.add_file(format!("{KB}/intents/python/isolate.toml"), RECORD);
        // The manifest still names the id, so the compiled entry is found;
        // the catalog lookup by id alone would be ambiguous.
        let report = fixture.explain("kb.intent.isolate", &["rust"]).unwrap();
        assert_eq!(report.intent.key, "rust/isolate");
        assert_eq!(report.intent.resolution, RecordResolution::Key);
    }

    #[test]
    fn the_key_wins_over_a_shared_id() {
        let fixture = Fixture::new();
        fixture.fs.add_file(
            format!("{KB}/intents/python/isolate.toml"),
            RECORD.replace("Isolate the functional core", "Python twin"),
        );
        let report = fixture.explain("rust/isolate", &["rust"]).unwrap();
        assert_eq!(report.intent.resolution, RecordResolution::Key);
        assert_eq!(report.intent.title.as_deref(), Some("Isolate the functional core"));
    }

    #[test]
    fn moved_record_resolves_by_its_unique_id() {
        let mut fixture = Fixture::new();
        fixture.manifest.intents[1].key = "rust/old-home".to_string();
        let report = fixture.explain("rust/old-home", &["rust"]).unwrap();
        assert_eq!(report.intent.resolution, RecordResolution::Id);
        assert_eq!(report.intent.resolution_detail, None);
        assert_eq!(report.intent.title.as_deref(), Some("Isolate the functional core"));
        assert_eq!(report.intent.key, "rust/old-home", "the manifest key stays the locator");
    }

    #[test]
    fn moved_record_with_a_shared_id_is_not_found_and_names_the_records() {
        let mut fixture = Fixture::new();
        fixture.fs.add_file(format!("{KB}/intents/python/isolate.toml"), RECORD);
        fixture.manifest.intents[1].key = "rust/old-home".to_string();
        let report = fixture.explain("rust/old-home", &["rust"]).unwrap();
        assert_eq!(report.intent.resolution, RecordResolution::NotFound);
        assert_eq!(
            report.intent.resolution_detail.as_deref(),
            Some(
                "no record at key rust/old-home, and id kb.intent.isolate is shared by 2 records (python/isolate, rust/isolate); re-run cmf install to recompile"
            )
        );
        assert_eq!(report.intent.title, None);
        assert!(report.intent.validators.is_empty());
    }

    #[test]
    fn renamed_id_still_resolves_by_key_and_missing_record_is_not_found() {
        let mut fixture = Fixture::new();
        fixture.manifest.intents[1].id = "kb.intent.renamed".to_string();
        let report = fixture.explain("rust/isolate", &["rust"]).unwrap();
        assert_eq!(report.intent.resolution, RecordResolution::Key);
        assert_eq!(
            report.intent.id.as_deref(),
            Some("kb.intent.renamed"),
            "the manifest id is kept"
        );

        let report = fixture.explain("rust/other", &["rust"]).unwrap();
        assert_eq!(report.intent.resolution, RecordResolution::NotFound);
        assert_eq!(report.intent.resolution_detail, None);
        assert_eq!(report.intent.title, None);
        assert!(report.intent.validators.is_empty());
        assert!(!report.intent.stale, "nothing to compare");
        assert_eq!(report.intent.config, json!({}));
    }

    #[test]
    fn stale_follows_the_record_checksum() {
        let mut fixture = Fixture::new();
        fixture.manifest.intents[1].checksum = "sha256:stale".to_string();
        assert!(fixture.explain("rust/isolate", &["rust"]).unwrap().intent.stale);
    }

    #[test]
    fn pinned_tree_argv_uses_the_placeholder_and_stale_compares_head() {
        let fixture = Fixture::new();
        for (path, bytes) in fixture.fs.snapshot_files() {
            if let Ok(relative) = path.strip_prefix(KB) {
                fixture.fs.add_file(Path::new(PINNED).join(relative), bytes);
            }
        }
        fixture.fs.add_dir(format!("{PINNED}/intents"));
        fixture.fs.add_file(format!("{KB}/checks/rust/isolate.sh"), "corrected at HEAD");
        let trees = Trees {
            verified: Path::new(PINNED),
            working: Some(Path::new(KB)),
        };
        let report = fixture
            .explain_with("rust/isolate", &["rust"], trees, Path::new(PINNED))
            .unwrap();
        assert!(report.intent.stale, "the validator changed at HEAD");
        assert_eq!(
            report.intent.validators[0].argv.as_ref().unwrap()[0],
            "<pinned-tree>/checks/rust/isolate.sh"
        );
    }

    #[test]
    fn dropped_only_intent_is_unguided_with_its_record_when_present() {
        let fixture = Fixture::new();
        fixture.fs.add_file(
            format!("{KB}/intents/rust/budgeted.toml"),
            RECORD.replace("kb.intent.isolate", "kb.intent.budgeted"),
        );
        let report = fixture.explain("rust/budgeted", &["rust"]).unwrap();
        let intent = report.intent;
        assert!(!intent.compiled);
        assert!(intent.dropped);
        assert_eq!(intent.drop_reason.as_deref(), Some("budget"));
        assert_eq!(intent.id.as_deref(), Some("kb.intent.budgeted"));
        assert_eq!(intent.resolution, RecordResolution::Key);
        assert!(!intent.stale);
        assert_eq!(intent.validators.len(), 2);
        assert!(intent.validators.iter().all(|plan| !plan.would_run && plan.argv.is_none()));
        assert!(intent.validators[0].skipped.as_deref().unwrap().contains("dropped (budget)"));

        fixture
            .fs
            .remove_file(Path::new(&format!("{KB}/intents/rust/budgeted.toml")))
            .unwrap();
        let report = fixture.explain("rust/budgeted", &["rust"]).unwrap();
        assert_eq!(report.intent.resolution, RecordResolution::NotFound);
        assert_eq!(report.intent.id, None);
        assert!(report.intent.validators.is_empty());
    }

    #[test]
    fn dropped_intent_is_found_by_id_through_its_record() {
        let fixture = Fixture::new();
        fixture.fs.add_file(
            format!("{KB}/intents/rust/budgeted.toml"),
            RECORD.replace("kb.intent.isolate", "kb.intent.budgeted"),
        );
        let report = fixture.explain("kb.intent.budgeted", &["rust"]).unwrap();
        assert_eq!(report.intent.key, "rust/budgeted");
        assert!(report.intent.dropped);
    }

    #[test]
    fn intent_both_compiled_and_dropped_reports_both() {
        let mut fixture = Fixture::new();
        fixture.manifest.dropped.push(DroppedIntent {
            key: "rust/isolate".to_string(),
            reason: "budget".to_string(),
        });
        let intent = fixture.explain("rust/isolate", &["rust"]).unwrap().intent;
        assert!(intent.compiled);
        assert!(intent.dropped);
        assert_eq!(intent.drop_reason.as_deref(), Some("budget"));
        assert!(intent.validators[0].would_run, "the compiled entry still runs");
    }

    #[test]
    fn intent_not_in_the_manifest_is_an_error() {
        let fixture = Fixture::new();
        let error = fixture.explain("rust/unknown", &["rust"]).unwrap_err().to_string();
        assert!(error.contains("\"rust/unknown\" is not in the manifest"), "{error}");
        let error = fixture.explain("kb.intent.unknown", &["rust"]).unwrap_err().to_string();
        assert!(error.contains("not in the manifest"), "{error}");
    }

    #[test]
    fn json_shape_is_documented() {
        let fixture = Fixture::new();
        let report = fixture.explain("rust/isolate", &["python"]).unwrap();
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["intent"]["resolution"], "key");
        assert_eq!(value["intent"]["resolution_detail"], Value::Null);
        assert_eq!(value["intent"]["compiled"], true);
        assert_eq!(value["intent"]["dropped"], false);
        assert_eq!(value["intent"]["drop_reason"], Value::Null);
        assert_eq!(value["intent"]["validators"][0]["would_run"], false);
        assert_eq!(value["intent"]["validators"][1]["would_run"], true);
        assert_eq!(value["intent"]["validators"][1]["skipped"], Value::Null);
        assert_eq!(value["atlas"]["verified_against"], "head");
    }
}
