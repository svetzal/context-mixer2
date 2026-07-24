//! Global config-root JSON documents: `sources.json`, `sets.json`, and `config.json`.
//!
//! Every function here loads or saves one of these documents through the
//! [`Filesystem`] gateway and [`ConfigPaths`] location resolver, via the generic
//! helpers in [`crate::json_file`]. `mutate_sources`/`mutate_sets` compose a
//! load-mutate-save round trip so callers never forget to persist a change. The
//! `installed` submodule (re-exported here) holds the per-artifact installed-config
//! record type.

use std::path::PathBuf;

use crate::error::{CmxError, Result};
use crate::gateway::filesystem::Filesystem;
use crate::paths::ConfigPaths;
use crate::types::{CmxConfig, InstallScope, SetsFile, SourceEntry, SourceType, SourcesFile};

mod installed;
pub use installed::*;

// ---------------------------------------------------------------------------
// Testable variants (accept injected Filesystem + ConfigPaths)
// ---------------------------------------------------------------------------

/// Load `sources.json`, or the default empty [`SourcesFile`] if it does not exist.
pub fn load_sources(fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<SourcesFile> {
    crate::json_file::load_json(&paths.sources_path(), fs)
}

/// Write `sources.json`, replacing its previous contents.
pub fn save_sources(sources: &SourcesFile, fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<()> {
    crate::json_file::save_json(sources, &paths.sources_path(), fs)
}

/// Load, mutate via `f`, and save the `SourcesFile` in one step.
///
/// `f` is called with a mutable reference to the in-memory sources and may
/// return `Err` to abort without writing; on success the file is saved and
/// the value returned by `f` is propagated.
///
/// The closure and outer return are generic over `E: From<CmxError>`, which
/// keeps the leaf errors (`load_sources`/`save_sources`) as the crate's typed
/// `CmxError` while still letting application closures return their own error
/// type (e.g. `CliError`) via the `From<CmxError>` blanket in cmx.  A pure-
/// library caller that passes `E = CmxError` gets full `.code()` discriminants
/// with no type erasure.
pub fn mutate_sources<F, T, E>(
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
    f: F,
) -> core::result::Result<T, E>
where
    F: FnOnce(&mut SourcesFile) -> core::result::Result<T, E>,
    E: From<CmxError>,
{
    let mut sources = load_sources(fs, paths)?;
    let result = f(&mut sources)?;
    save_sources(&sources, fs, paths)?;
    Ok(result)
}

/// Load the `sets.json` for the given scope, or the default empty [`SetsFile`] if
/// it does not exist.
pub fn load_sets(
    scope: InstallScope,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<SetsFile> {
    crate::json_file::load_json(&paths.sets_path(scope), fs)
}

/// Write the `sets.json` for the given scope, replacing its previous contents.
pub fn save_sets(
    sets: &SetsFile,
    scope: InstallScope,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<()> {
    crate::json_file::save_json(sets, &paths.sets_path(scope), fs)
}

/// Load, mutate via `f`, and save the `SetsFile` for the given scope in one step.
///
/// `f` is called with a mutable reference to the in-memory sets and may
/// return `Err` to abort without writing; on success the file is saved and
/// the value returned by `f` is propagated.
///
/// See [`mutate_sources`] for the `E: From<CmxError>` design rationale: leaf
/// errors are the crate's typed `CmxError` and are never erased; callers choose
/// their own error type as `E`.
pub fn mutate_sets<F, T, E>(
    scope: InstallScope,
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
    f: F,
) -> core::result::Result<T, E>
where
    F: FnOnce(&mut SetsFile) -> core::result::Result<T, E>,
    E: From<CmxError>,
{
    let mut sets = load_sets(scope, fs, paths)?;
    let result = f(&mut sets)?;
    save_sets(&sets, scope, fs, paths)?;
    Ok(result)
}

/// Load `config.json`, or the default [`CmxConfig`] if it does not exist.
pub fn load_config(fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<CmxConfig> {
    crate::json_file::load_json(&paths.config_path(), fs)
}

/// Write `config.json`, replacing its previous contents.
pub fn save_config(config: &CmxConfig, fs: &dyn Filesystem, paths: &ConfigPaths) -> Result<()> {
    crate::json_file::save_json(config, &paths.config_path(), fs)
}

/// The explicit set of platforms the user has told cmx to manage, if any.
///
/// Returns `Some(list)` when `config.platforms` is non-empty (the authoritative
/// managed set), or `None` when unset — signalling callers to fall back to their
/// own default (every supported platform for `doctor`/`uninstall`; the in-use
/// inference for `install`).
pub fn managed_platforms(
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Option<Vec<crate::platform::Platform>>> {
    let cfg = load_config(fs, paths)?;
    Ok((!cfg.platforms.is_empty()).then_some(cfg.platforms))
}

/// The platforms a default (no `--platform`) cross-platform command considers:
/// the explicit managed set when one is configured, otherwise every supported
/// platform.
///
/// This is the shared "managed-or-all" fallback used by `uninstall`, `sync`, and
/// `diff`. Callers still filter by [`Platform::supports`](crate::platform::Platform::supports)
/// for the relevant kind. (`install` deliberately differs — with no managed set
/// it infers the platforms already in use rather than falling back to all.)
pub fn managed_or_all_platforms(
    fs: &dyn Filesystem,
    paths: &ConfigPaths,
) -> Result<Vec<crate::platform::Platform>> {
    Ok(managed_platforms(fs, paths)?.unwrap_or_else(|| crate::platform::Platform::ALL.to_vec()))
}

/// Resolve the effective canonical artifact home: the `home` override in the
/// config if set, otherwise the default under the config root.
pub fn resolve_artifact_home(config: &CmxConfig, paths: &ConfigPaths) -> PathBuf {
    config.home.clone().unwrap_or_else(|| paths.default_artifact_home())
}

/// Expand a leading `~` in a config path entry against the OS home directory.
fn expand_tilde(entry: &str, home_dir: &std::path::Path) -> PathBuf {
    if let Some(rest) = entry.strip_prefix("~/") {
        home_dir.join(rest)
    } else if entry == "~" {
        home_dir.to_path_buf()
    } else {
        PathBuf::from(entry)
    }
}

/// Resolve the on-disk path a local or git-cloned source entry reads from.
///
/// Returns [`CmxError::SourcePathMissing`] if the entry's type-specific path field
/// (`path` for [`SourceType::Local`], `local_clone` for [`SourceType::Git`]) is unset.
pub fn resolve_local_path(entry: &SourceEntry) -> Result<PathBuf> {
    match entry.source_type {
        SourceType::Local => entry.path.clone().ok_or(CmxError::SourcePathMissing {
            msg: "Local source has no path configured",
            kind: SourceType::Local,
        }),
        SourceType::Git => entry.local_clone.clone().ok_or(CmxError::SourcePathMissing {
            msg: "Git source has no local clone path configured",
            kind: SourceType::Git,
        }),
    }
}

// ---------------------------------------------------------------------------
// Unit tests (use FakeFilesystem + ConfigPaths::for_test)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{make_local_entry, test_paths};

    // --- load_sources_with ---

    #[test]
    fn load_sources_returns_default_when_file_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources = load_sources(&fs, &paths).unwrap();
        assert!(sources.sources.is_empty());
        assert_eq!(sources.version, 1);
    }

    #[test]
    fn load_sources_parses_valid_json() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let json = r#"{"version":1,"sources":{"my-source":{"type":"local","path":"/some/path","last_updated":"2024-01-01T00:00:00Z"}}}"#;
        fs.add_file(paths.sources_path(), json);
        let sources = load_sources(&fs, &paths).unwrap();
        assert!(sources.sources.contains_key("my-source"));
    }

    #[test]
    fn load_sources_returns_error_on_malformed_json() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        fs.add_file(paths.sources_path(), "not valid json{{{{");
        let result = load_sources(&fs, &paths);
        assert!(result.is_err());
    }

    #[test]
    fn save_sources_creates_parent_dirs_and_writes_json() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources = SourcesFile::default();
        save_sources(&sources, &fs, &paths).unwrap();
        assert!(fs.file_exists(&paths.sources_path()));
    }

    // --- mutate_sources_with ---

    #[test]
    fn mutate_sources_with_loads_applies_and_saves() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        mutate_sources(&fs, &paths, |sources| -> Result<()> {
            sources
                .sources
                .insert("test-source".to_string(), make_local_entry("/path", None));
            Ok(())
        })
        .unwrap();

        let loaded = load_sources(&fs, &paths).unwrap();
        assert!(loaded.sources.contains_key("test-source"));
    }

    #[test]
    fn mutate_sources_with_does_not_save_on_closure_error() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let result: anyhow::Result<()> =
            mutate_sources(&fs, &paths, |_sources| Err(anyhow::anyhow!("closure error")));
        assert!(result.is_err());

        let loaded = load_sources(&fs, &paths).unwrap();
        assert!(loaded.sources.is_empty(), "sources should not be saved after closure error");
    }

    #[test]
    fn load_and_save_sources_round_trip() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let mut sources = SourcesFile::default();
        sources
            .sources
            .insert("test-source".to_string(), make_local_entry("/some/path", None));

        save_sources(&sources, &fs, &paths).unwrap();
        let loaded = load_sources(&fs, &paths).unwrap();
        assert_eq!(loaded.sources.len(), 1);
        assert!(loaded.sources.contains_key("test-source"));
    }

    // --- load_sets / save_sets / mutate_sets ---

    #[test]
    fn load_sets_returns_default_when_file_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sets = load_sets(InstallScope::Global, &fs, &paths).unwrap();
        assert!(sets.sets.is_empty());
        assert_eq!(sets.version, 1);
    }

    #[test]
    fn mutate_sets_create_modify_save() {
        use crate::types::{SetDef, SetState};

        let fs = FakeFilesystem::new();
        let paths = test_paths();

        mutate_sets(InstallScope::Global, &fs, &paths, |sets| -> Result<()> {
            sets.sets.insert(
                "rust-work".to_string(),
                SetDef {
                    description: Some("desc".to_string()),
                    state: SetState::Inactive,
                    members: vec![],
                },
            );
            Ok(())
        })
        .unwrap();

        let loaded = load_sets(InstallScope::Global, &fs, &paths).unwrap();
        assert!(loaded.sets.contains_key("rust-work"));
    }

    #[test]
    fn mutate_sets_does_not_save_on_closure_error() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();

        let result: anyhow::Result<()> = mutate_sets(InstallScope::Global, &fs, &paths, |_sets| {
            Err(anyhow::anyhow!("closure error"))
        });
        assert!(result.is_err());

        let loaded = load_sets(InstallScope::Global, &fs, &paths).unwrap();
        assert!(loaded.sets.is_empty(), "sets should not be saved after closure error");
    }

    #[test]
    fn load_and_save_sets_round_trip_local_scope() {
        use crate::types::{SetDef, SetState};

        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let mut sets = SetsFile::default();
        sets.sets.insert(
            "blog".to_string(),
            SetDef {
                description: None,
                state: SetState::Active,
                members: vec![],
            },
        );
        save_sets(&sets, InstallScope::Local, &fs, &paths).unwrap();
        let loaded = load_sets(InstallScope::Local, &fs, &paths).unwrap();
        assert_eq!(loaded.sets.len(), 1);
        assert!(loaded.sets.contains_key("blog"));
        // Global scope remains unaffected
        assert!(load_sets(InstallScope::Global, &fs, &paths).unwrap().sets.is_empty());
    }

    // --- load_config_with / save_config_with ---

    #[test]
    fn load_config_returns_default_when_absent() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let cfg = load_config(&fs, &paths).unwrap();
        assert_eq!(cfg.version, 1);
    }

    #[test]
    fn load_and_save_config_round_trip() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let mut cfg = CmxConfig::default();
        cfg.llm.model = "test-model".to_string();
        save_config(&cfg, &fs, &paths).unwrap();
        let loaded = load_config(&fs, &paths).unwrap();
        assert_eq!(loaded.llm.model, "test-model");
    }

    // --- failure-path tests ---

    #[test]
    fn save_sources_returns_error_when_filesystem_write_fails() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources_path = paths.sources_path();

        // save_json writes to a sibling .tmp file first — fail that write
        fs.set_fail_on_write(crate::json_file::tmp_path(&sources_path));

        let sources = SourcesFile::default();
        let result = save_sources(&sources, &fs, &paths);
        assert!(result.is_err(), "expected Err when sources file write fails");

        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Failed to write"), "expected 'Failed to write' in error: {msg}");
    }

    // --- resolve_local_path ---

    #[test]
    fn resolve_local_path_errors_for_local_entry_with_no_path() {
        use crate::types::{SourceEntry, SourceType};
        let entry = SourceEntry {
            source_type: SourceType::Local,
            path: None,
            url: None,
            local_clone: None,
            branch: None,
            last_updated: None,
        };
        let result = resolve_local_path(&entry);
        assert!(result.is_err(), "expected Err when Local source has no path");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no path configured"),
            "expected 'no path configured' in error: {msg}"
        );
    }

    #[test]
    fn resolve_local_path_errors_for_git_entry_with_no_local_clone() {
        use crate::types::{SourceEntry, SourceType};
        let entry = SourceEntry {
            source_type: SourceType::Git,
            path: None,
            url: Some("https://github.com/example/repo.git".to_string()),
            local_clone: None,
            branch: None,
            last_updated: None,
        };
        let result = resolve_local_path(&entry);
        assert!(result.is_err(), "expected Err when Git source has no local clone");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no local clone"), "expected 'no local clone' in error: {msg}");
    }

    #[test]
    fn resolve_local_path_returns_path_for_local_entry() {
        let entry = make_local_entry("/some/path", None);
        let result = resolve_local_path(&entry);
        assert!(result.is_ok(), "expected Ok for local entry with path");
        assert_eq!(result.unwrap(), std::path::PathBuf::from("/some/path"));
    }

    // --- typed-error regression: mutate_sources with E = CmxError preserves code() ---

    #[test]
    fn mutate_sources_typed_error_preserves_code_on_save_failure() {
        let fs = FakeFilesystem::new();
        let paths = test_paths();
        let sources_path = paths.sources_path();

        // Force the save (tmp-file write) to fail so the leaf CmxError propagates.
        fs.set_fail_on_write(crate::json_file::tmp_path(&sources_path));

        // Use E = CmxError directly — no anyhow erasure.
        let result: Result<()> = mutate_sources(&fs, &paths, |_sources| -> Result<()> { Ok(()) });
        assert!(result.is_err(), "expected Err when sources save fails");
        let err = result.unwrap_err();
        assert!(
            !err.code().is_empty(),
            "CmxError must carry a non-empty .code() discriminant; got: {err:?}"
        );
        assert!(
            matches!(err, CmxError::Io { .. }),
            "expected CmxError::Io variant for filesystem write failure; got: {err:?}"
        );
    }
}
