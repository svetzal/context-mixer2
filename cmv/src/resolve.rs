//! Knowledge-base resolution: which directory holds the intent records and
//! validators, and how that answer was reached (see `CMV.md`, design decision
//! 5, "The knowledge base is the registry, pinned by revision").
//!
//! The order is fixed: a `--knowledge-base <path>` override wins; otherwise
//! the manifest's `knowledge_base.source` is looked up in the cmx sources
//! registry (`sources.json`, through the `Filesystem` gateway and
//! `ConfigPaths`); otherwise the manifest's recorded `knowledge_base.path` is
//! used. A source name the registry does not know falls through to the path
//! with a warning, because the manifest still says where the records were when
//! cmf read them. Pure over the gateways: nothing here prints or exits.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cmf::manifest::KnowledgeBase;
use cmx_core::config;
use cmx_core::gateway::Filesystem;
use cmx_core::paths::ConfigPaths;
use serde::Serialize;

/// Which step of the resolution order produced the knowledge-base root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedBy {
    /// `--knowledge-base <path>` on the command line.
    Override,
    /// The manifest's `knowledge_base.source`, found in the cmx registry.
    Source,
    /// The manifest's `knowledge_base.path`.
    Path,
}

/// Where the knowledge base is and how cmv decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The knowledge-base root, as resolved (not canonicalized).
    pub path: PathBuf,
    /// The step that produced `path`.
    pub resolved_by: ResolvedBy,
    /// The cmx source name the manifest recorded, whether or not the registry
    /// knew it.
    pub source: Option<String>,
    /// A condition worth telling the user about that did not stop resolution:
    /// today, a recorded source name missing from the registry.
    pub warning: Option<String>,
}

/// Resolve the knowledge-base root per the order in the module docs.
///
/// Only the source step touches the registry, so an override never fails on
/// an unreadable `sources.json`.
pub fn resolve(
    override_path: Option<&Path>,
    recorded: &KnowledgeBase,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Resolution> {
    let source = recorded.source.clone();
    if let Some(path) = override_path {
        return Ok(Resolution {
            path: path.to_path_buf(),
            resolved_by: ResolvedBy::Override,
            source,
            warning: None,
        });
    }
    let Some(name) = source.as_deref() else {
        return Ok(from_path(recorded, source, None));
    };
    let sources =
        config::load_sources(fs, paths).context("could not read the cmx sources registry")?;
    match sources.sources.get(name).map(config::resolve_local_path) {
        Some(Ok(path)) => Ok(Resolution {
            path,
            resolved_by: ResolvedBy::Source,
            source,
            warning: None,
        }),
        Some(Err(error)) => {
            let warning = format!(
                "cmx source {name:?} has no local directory ({error}); using the path the manifest recorded, {}",
                recorded.path.display()
            );
            Ok(from_path(recorded, source, Some(warning)))
        }
        None => {
            let warning = format!(
                "cmx source {name:?} is not registered; using the path the manifest recorded, {}. Register it with `cmx source add` to resolve through the registry",
                recorded.path.display()
            );
            Ok(from_path(recorded, source, Some(warning)))
        }
    }
}

fn from_path(
    recorded: &KnowledgeBase,
    source: Option<String>,
    warning: Option<String>,
) -> Resolution {
    Resolution {
        path: recorded.path.clone(),
        resolved_by: ResolvedBy::Path,
        source,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::fakes::FakeFilesystem;
    use cmx_core::test_support::{make_git_entry, make_local_entry, test_paths};
    use cmx_core::types::{SourceEntry, SourceType, SourcesFile};

    fn recorded(source: Option<&str>) -> KnowledgeBase {
        KnowledgeBase {
            source: source.map(str::to_string),
            path: PathBuf::from("/recorded/kb"),
            revision: None,
        }
    }

    fn registry(fs: &FakeFilesystem, entries: &[(&str, SourceEntry)]) -> ConfigPaths {
        let paths = test_paths();
        let mut sources = SourcesFile::default();
        for (name, entry) in entries {
            sources.sources.insert((*name).to_string(), entry.clone());
        }
        config::save_sources(&sources, fs, &paths).unwrap();
        paths
    }

    #[test]
    fn override_wins_without_consulting_the_registry() {
        let fs = FakeFilesystem::new();
        let paths = registry(&fs, &[("guidelines", make_local_entry("/registered/kb", None))]);
        let resolution =
            resolve(Some(Path::new("../kb")), &recorded(Some("guidelines")), &fs, &paths).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                path: PathBuf::from("../kb"),
                resolved_by: ResolvedBy::Override,
                source: Some("guidelines".to_string()),
                warning: None,
            }
        );
    }

    #[test]
    fn override_does_not_need_a_readable_registry() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        fs.add_file(paths.sources_path(), "not json");
        let resolution =
            resolve(Some(Path::new("/kb")), &recorded(Some("x")), &fs, &paths).unwrap();
        assert_eq!(resolution.resolved_by, ResolvedBy::Override);
    }

    #[test]
    fn registered_local_source_resolves_to_its_path() {
        let fs = FakeFilesystem::new();
        let paths = registry(&fs, &[("guidelines", make_local_entry("/registered/kb", None))]);
        let resolution = resolve(None, &recorded(Some("guidelines")), &fs, &paths).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                path: PathBuf::from("/registered/kb"),
                resolved_by: ResolvedBy::Source,
                source: Some("guidelines".to_string()),
                warning: None,
            }
        );
    }

    #[test]
    fn registered_git_source_resolves_to_its_clone() {
        let fs = FakeFilesystem::new();
        let paths = registry(
            &fs,
            &[(
                "remote",
                make_git_entry("https://example.com/kb.git", "/clones/kb", "main", None),
            )],
        );
        let resolution = resolve(None, &recorded(Some("remote")), &fs, &paths).unwrap();
        assert_eq!(resolution.path, PathBuf::from("/clones/kb"));
        assert_eq!(resolution.resolved_by, ResolvedBy::Source);
    }

    #[test]
    fn unregistered_source_falls_through_to_the_recorded_path_with_a_warning() {
        let fs = FakeFilesystem::new();
        let paths = registry(&fs, &[("other", make_local_entry("/other", None))]);
        let resolution = resolve(None, &recorded(Some("guidelines")), &fs, &paths).unwrap();
        assert_eq!(resolution.path, PathBuf::from("/recorded/kb"));
        assert_eq!(resolution.resolved_by, ResolvedBy::Path);
        assert_eq!(resolution.source.as_deref(), Some("guidelines"), "the recorded name is kept");
        let warning = resolution.warning.expect("a warning names the missing source");
        assert!(warning.contains("\"guidelines\" is not registered"), "{warning}");
        assert!(warning.contains("/recorded/kb"), "{warning}");
        assert!(warning.contains("cmx source add"), "{warning}");
    }

    #[test]
    fn empty_registry_also_falls_through() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let resolution = resolve(None, &recorded(Some("guidelines")), &fs, &paths).unwrap();
        assert_eq!(resolution.resolved_by, ResolvedBy::Path);
        assert!(resolution.warning.is_some());
    }

    #[test]
    fn source_without_a_local_directory_falls_through_with_a_warning() {
        let fs = FakeFilesystem::new();
        let uncloned = SourceEntry {
            source_type: SourceType::Git,
            path: None,
            url: Some("https://example.com/kb.git".to_string()),
            local_clone: None,
            branch: None,
            last_updated: None,
        };
        let paths = registry(&fs, &[("remote", uncloned)]);
        let resolution = resolve(None, &recorded(Some("remote")), &fs, &paths).unwrap();
        assert_eq!(resolution.resolved_by, ResolvedBy::Path);
        assert!(resolution.warning.unwrap().contains("no local directory"));
    }

    #[test]
    fn manifest_without_a_source_uses_its_path_silently() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let resolution = resolve(None, &recorded(None), &fs, &paths).unwrap();
        assert_eq!(
            resolution,
            Resolution {
                path: PathBuf::from("/recorded/kb"),
                resolved_by: ResolvedBy::Path,
                source: None,
                warning: None,
            }
        );
    }

    #[test]
    fn unreadable_registry_is_an_error_when_it_is_needed() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        fs.add_file(paths.sources_path(), "not json");
        let error = resolve(None, &recorded(Some("guidelines")), &fs, &paths).unwrap_err();
        assert!(format!("{error:#}").contains("cmx sources registry"));
    }

    #[test]
    fn resolved_by_serializes_in_snake_case() {
        assert_eq!(serde_json::to_value(ResolvedBy::Override).unwrap(), "override");
        assert_eq!(serde_json::to_value(ResolvedBy::Source).unwrap(), "source");
        assert_eq!(serde_json::to_value(ResolvedBy::Path).unwrap(), "path");
    }
}
