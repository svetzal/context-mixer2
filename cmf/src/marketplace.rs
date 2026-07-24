//! Marketplace validation and generation.

use anyhow::Result;
use cmx::gateway::Filesystem;
use cmx::json_file::{load_json, save_json};

use crate::plugin_types::{Marketplace, MarketplaceEntry, PluginManifest, PluginSource};
use crate::repo::{RepoRoot, resolve_source_path};
use crate::validation::{ValidationIssue, load_and_validate_json};

/// Validate marketplace.json against the actual plugin directories.
///
/// Checks that marketplace.json exists and is valid, that each listed plugin
/// source resolves to a real directory with a plugin.json, and that all plugin
/// directories are listed.
pub fn validate_marketplace(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");

    let (maybe_marketplace, early_issues) = load_and_validate_json::<Marketplace>(
        &marketplace_path,
        "marketplace",
        "marketplace.json",
        fs,
    )?;
    let Some(marketplace) = maybe_marketplace else {
        return Ok(early_issues);
    };

    let mut issues = Vec::new();

    // Check 3: each entry's source resolves to a directory with plugin.json
    issues.extend(validate_marketplace_entries(&marketplace.plugins, root, fs)?);

    // Check 4: unlisted plugins — directories in plugins/ with plugin.json not in marketplace
    issues.extend(find_unlisted_plugins(&marketplace, root, fs));

    Ok(issues)
}

/// Discover plugin entries by walking the plugins directory.
///
/// Loads each plugin's `plugin.json`, builds a `MarketplaceEntry`, and
/// preserves any existing category assignment by looking up the plugin name in
/// `existing_by_name`.
fn discover_plugin_entries(
    plugins_dir: &std::path::Path,
    existing_by_name: &std::collections::HashMap<&str, &MarketplaceEntry>,
    fs: &dyn Filesystem,
) -> Result<Vec<MarketplaceEntry>> {
    let mut entries = Vec::new();

    if !fs.is_dir(plugins_dir) {
        return Ok(entries);
    }

    let Ok(dir_entries) = fs.read_dir(plugins_dir) else {
        return Ok(entries);
    };

    for dir_entry in dir_entries {
        if !dir_entry.is_dir {
            continue;
        }
        let plugin_json = dir_entry.path.join(".claude-plugin").join("plugin.json");
        if !fs.exists(&plugin_json) {
            continue;
        }

        let manifest: PluginManifest = load_json(&plugin_json, fs)?;
        let source = PluginSource::Local(format!("./plugins/{}", dir_entry.file_name));

        // Preserve category from existing entry if the name matches
        let category =
            existing_by_name.get(manifest.name.as_str()).and_then(|e| e.category.clone());

        entries.push(MarketplaceEntry {
            name: manifest.name,
            description: manifest.description.unwrap_or_default(),
            source: Some(source),
            category,
            ..Default::default()
        });
    }

    Ok(entries)
}

/// Validate each marketplace entry's source directory, plugin.json existence,
/// and name consistency.
fn validate_marketplace_entries(
    entries: &[MarketplaceEntry],
    root: &RepoRoot,
    fs: &dyn Filesystem,
) -> Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();

    for entry in entries {
        let Some(local_source) = entry.source.as_ref().and_then(|s| s.as_local()) else {
            if let Some(src) = &entry.source {
                issues.push(ValidationIssue::warning(
                    "marketplace",
                    format!(
                        "plugin \"{}\" uses remote source type '{}' which is not yet supported for local validation",
                        entry.name,
                        src.source_type_name()
                    ),
                ));
            }
            continue;
        };
        let plugin_path = resolve_source_path(&root.path, local_source);

        if !fs.exists(&plugin_path) || !fs.is_dir(&plugin_path) {
            issues.push(ValidationIssue::error(
                "marketplace",
                format!(
                    "plugin \"{}\" source \"{}\" does not resolve to an existing directory",
                    entry.name, local_source
                ),
            ));
            continue;
        }

        let plugin_json = plugin_path.join(".claude-plugin").join("plugin.json");
        if !fs.exists(&plugin_json) {
            issues.push(ValidationIssue::warning(
                "marketplace",
                format!("plugin \"{}\" directory has no .claude-plugin/plugin.json", entry.name),
            ));
            continue;
        }

        // Check name match between marketplace entry and plugin.json; report
        // unreadable or malformed files as errors instead of silently skipping.
        let (maybe_manifest, read_issues) = load_and_validate_json::<PluginManifest>(
            &plugin_json,
            "marketplace",
            &format!("plugin \"{}\" plugin.json", entry.name),
            fs,
        )?;
        issues.extend(read_issues);
        if let Some(manifest) = maybe_manifest {
            if manifest.name != entry.name {
                issues.push(ValidationIssue::warning(
                    "marketplace",
                    format!(
                        "marketplace entry name \"{}\" does not match plugin.json name \"{}\"",
                        entry.name, manifest.name
                    ),
                ));
            }
        }
    }

    Ok(issues)
}

/// Scan for plugin directories that have a plugin.json but are not listed in
/// marketplace.json.
fn find_unlisted_plugins(
    marketplace: &Marketplace,
    root: &RepoRoot,
    fs: &dyn Filesystem,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    let plugins_dir = root.path.join("plugins");
    if !fs.is_dir(&plugins_dir) {
        return issues;
    }

    // A read_dir failure after is_dir succeeds is a genuine I/O error worth reporting.
    let entries = match fs.read_dir(&plugins_dir) {
        Ok(e) => e,
        Err(e) => {
            issues.push(ValidationIssue::error(
                "marketplace",
                format!("could not read plugins directory: {e}"),
            ));
            return issues;
        }
    };

    let listed_sources: Vec<_> = marketplace
        .plugins
        .iter()
        .filter_map(|e| e.source.as_ref()?.as_local().map(|s| resolve_source_path(&root.path, s)))
        .collect();

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let plugin_json = entry.path.join(".claude-plugin").join("plugin.json");
        if fs.exists(&plugin_json) && !listed_sources.contains(&entry.path) {
            issues.push(ValidationIssue::warning(
                "marketplace",
                format!(
                    "unlisted plugin \"{}\" has plugin.json but is not in marketplace.json",
                    entry.file_name
                ),
            ));
        }
    }

    issues
}

/// Generate marketplace.json from the plugin directories.
///
/// Preserves existing `name`, `owner`, and `metadata` fields if marketplace.json
/// already exists. Discovers all plugins under `plugins/` that have a
/// `.claude-plugin/plugin.json`, and preserves existing category assignments for
/// matching entries.
pub fn generate_marketplace(root: &RepoRoot, fs: &dyn Filesystem) -> Result<usize> {
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");

    // Load existing marketplace to preserve top-level fields and categories
    let existing: Marketplace = if fs.exists(&marketplace_path) {
        load_json(&marketplace_path, fs)?
    } else {
        Marketplace::default()
    };

    // Build a lookup of existing entries by name for category preservation
    let existing_by_name: std::collections::HashMap<&str, &MarketplaceEntry> =
        existing.plugins.iter().map(|e| (e.name.as_str(), e)).collect();

    let plugins_dir = root.path.join("plugins");
    let mut entries = discover_plugin_entries(&plugins_dir, &existing_by_name, fs)?;

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let marketplace = Marketplace {
        name: existing.name,
        owner: existing.owner,
        metadata: existing.metadata,
        plugins: entries,
    };

    let plugin_count = marketplace.plugins.len();
    save_json(&marketplace, &marketplace_path, fs)?;
    Ok(plugin_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_types::{MarketplaceMetadata, Owner};
    use crate::repo::{RepoKind, RepoRoot};
    use crate::test_support::{fake_marketplace_json, fake_marketplace_root, fake_plugin_json};
    use crate::validation::IssueLevel;
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_marketplace_ok() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("alpha", "Alpha plugin", "./plugins/alpha")]);
        let root = fake_marketplace_root(&fs, &json);

        fs.add_dir("/repo/plugins/alpha");
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));

        let issues = validate_marketplace(&root, &fs).unwrap();
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_marketplace_missing_plugin_dir() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("ghost", "Ghost plugin", "./plugins/ghost")]);
        let root = fake_marketplace_root(&fs, &json);
        // No directory created for "ghost"

        let issues = validate_marketplace(&root, &fs).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].level, IssueLevel::Error);
        assert!(
            issues[0].message.contains("does not resolve"),
            "unexpected message: {}",
            issues[0].message
        );
    }

    #[test]
    fn validate_marketplace_unlisted_plugin() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("listed", "Listed plugin", "./plugins/listed")]);
        let root = fake_marketplace_root(&fs, &json);

        fs.add_dir("/repo/plugins/listed");
        fs.add_file("/repo/plugins/listed/.claude-plugin/plugin.json", fake_plugin_json("listed"));

        // Extra plugin not in marketplace.json
        fs.add_dir("/repo/plugins/unlisted");
        fs.add_file(
            "/repo/plugins/unlisted/.claude-plugin/plugin.json",
            fake_plugin_json("unlisted"),
        );

        let issues = validate_marketplace(&root, &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("unlisted"),
            "unexpected message: {}",
            warnings[0].message
        );
    }

    #[test]
    fn validate_marketplace_name_mismatch() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("wrong-name", "A plugin", "./plugins/my-plugin")]);
        let root = fake_marketplace_root(&fs, &json);

        fs.add_dir("/repo/plugins/my-plugin");
        fs.add_file(
            "/repo/plugins/my-plugin/.claude-plugin/plugin.json",
            fake_plugin_json("actual-name"),
        );

        let issues = validate_marketplace(&root, &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert!(
            warnings.iter().any(|w| w.message.contains("does not match")),
            "expected name mismatch warning, got: {warnings:?}"
        );
    }

    #[test]
    fn validate_marketplace_malformed_plugin_json() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("alpha", "Alpha plugin", "./plugins/alpha")]);
        let root = fake_marketplace_root(&fs, &json);

        fs.add_dir("/repo/plugins/alpha");
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", "{ not json");

        let issues = validate_marketplace(&root, &fs).unwrap();
        let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
        assert!(
            errors.iter().any(|e| e.message.contains("malformed")),
            "expected malformed error, got: {issues:?}"
        );
    }

    #[test]
    fn validate_marketplace_unreadable_plugin_json() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("alpha", "Alpha plugin", "./plugins/alpha")]);
        let root = fake_marketplace_root(&fs, &json);

        fs.add_dir("/repo/plugins/alpha");
        // Invalid UTF-8 bytes cause read_to_string to fail while exists() is true
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", vec![0xff, 0xfe]);

        let issues = validate_marketplace(&root, &fs).unwrap();
        let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
        assert!(
            errors.iter().any(|e| e.message.contains("could not be read")),
            "expected unreadable error, got: {issues:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Generation tests
    // -----------------------------------------------------------------------

    #[test]
    fn generate_marketplace_from_plugins() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo/.claude-plugin");
        fs.add_dir("/repo/plugins");

        fs.add_dir("/repo/plugins/beta");
        fs.add_file("/repo/plugins/beta/.claude-plugin/plugin.json", fake_plugin_json("beta"));
        fs.add_dir("/repo/plugins/alpha");
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        generate_marketplace(&root, &fs).unwrap();

        let content = fs
            .read_to_string(&PathBuf::from("/repo/.claude-plugin/marketplace.json"))
            .unwrap();
        let mp: Marketplace = serde_json::from_str(&content).unwrap();

        assert_eq!(mp.plugins.len(), 2);
        // Sorted by name
        assert_eq!(mp.plugins[0].name, "alpha");
        assert_eq!(
            mp.plugins[0].source.as_ref().and_then(|s| s.as_local()),
            Some("./plugins/alpha")
        );
        assert_eq!(mp.plugins[1].name, "beta");
        assert_eq!(
            mp.plugins[1].source.as_ref().and_then(|s| s.as_local()),
            Some("./plugins/beta")
        );
    }

    #[test]
    fn generate_marketplace_preserves_metadata() {
        let fs = FakeFilesystem::new();

        let existing = Marketplace {
            name: "my-marketplace".to_string(),
            owner: Some(Owner {
                name: "Test Owner".to_string(),
                email: "test@example.com".to_string(),
            }),
            metadata: Some(MarketplaceMetadata {
                description: Some("A curated marketplace".to_string()),
                version: Some("2.0.0".to_string()),
            }),
            plugins: vec![],
        };
        let existing_json = serde_json::to_string_pretty(&existing).unwrap();
        fs.add_file("/repo/.claude-plugin/marketplace.json", existing_json.as_str());
        fs.add_dir("/repo/plugins");

        fs.add_dir("/repo/plugins/only-plugin");
        fs.add_file(
            "/repo/plugins/only-plugin/.claude-plugin/plugin.json",
            fake_plugin_json("only-plugin"),
        );

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        generate_marketplace(&root, &fs).unwrap();

        let content = fs
            .read_to_string(&PathBuf::from("/repo/.claude-plugin/marketplace.json"))
            .unwrap();
        let mp: Marketplace = serde_json::from_str(&content).unwrap();

        assert_eq!(mp.name, "my-marketplace");
        assert_eq!(mp.owner.as_ref().unwrap().name, "Test Owner");
        assert_eq!(mp.metadata.as_ref().unwrap().version.as_deref(), Some("2.0.0"));
        assert_eq!(mp.plugins.len(), 1);
        assert_eq!(mp.plugins[0].name, "only-plugin");
    }

    #[test]
    fn generate_marketplace_preserves_categories() {
        let fs = FakeFilesystem::new();

        let existing = Marketplace {
            name: "test".to_string(),
            owner: None,
            metadata: None,
            plugins: vec![MarketplaceEntry {
                name: "alpha".to_string(),
                description: "Old description".to_string(),
                source: Some(PluginSource::Local("./plugins/alpha".to_string())),
                category: Some("ecosystem".to_string()),
                ..Default::default()
            }],
        };
        let existing_json = serde_json::to_string_pretty(&existing).unwrap();
        fs.add_file("/repo/.claude-plugin/marketplace.json", existing_json.as_str());
        fs.add_dir("/repo/plugins");

        fs.add_dir("/repo/plugins/alpha");
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        generate_marketplace(&root, &fs).unwrap();

        let content = fs
            .read_to_string(&PathBuf::from("/repo/.claude-plugin/marketplace.json"))
            .unwrap();
        let mp: Marketplace = serde_json::from_str(&content).unwrap();

        assert_eq!(mp.plugins.len(), 1);
        assert_eq!(mp.plugins[0].category.as_deref(), Some("ecosystem"));
    }

    #[test]
    fn generate_marketplace_adds_new_plugins() {
        let fs = FakeFilesystem::new();

        let existing = Marketplace {
            name: "test".to_string(),
            owner: None,
            metadata: None,
            plugins: vec![MarketplaceEntry {
                name: "existing".to_string(),
                description: "Already there".to_string(),
                source: Some(PluginSource::Local("./plugins/existing".to_string())),
                category: Some("tools".to_string()),
                ..Default::default()
            }],
        };
        let existing_json = serde_json::to_string_pretty(&existing).unwrap();
        fs.add_file("/repo/.claude-plugin/marketplace.json", existing_json.as_str());
        fs.add_dir("/repo/plugins");

        // Existing plugin still present
        fs.add_dir("/repo/plugins/existing");
        fs.add_file(
            "/repo/plugins/existing/.claude-plugin/plugin.json",
            fake_plugin_json("existing"),
        );

        // New plugin added
        fs.add_dir("/repo/plugins/brand-new");
        fs.add_file(
            "/repo/plugins/brand-new/.claude-plugin/plugin.json",
            fake_plugin_json("brand-new"),
        );

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        generate_marketplace(&root, &fs).unwrap();

        let content = fs
            .read_to_string(&PathBuf::from("/repo/.claude-plugin/marketplace.json"))
            .unwrap();
        let mp: Marketplace = serde_json::from_str(&content).unwrap();

        assert_eq!(mp.plugins.len(), 2);
        // Sorted by name
        assert_eq!(mp.plugins[0].name, "brand-new");
        assert!(mp.plugins[0].category.is_none()); // new plugin, no category
        assert_eq!(mp.plugins[1].name, "existing");
        assert_eq!(mp.plugins[1].category.as_deref(), Some("tools"));
    }
}
