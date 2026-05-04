use anyhow::Result;
use cmx::gateway::Filesystem;
use cmx::json_file::{load_json, save_json};

use crate::plugin_types::{Marketplace, MarketplaceEntry, PluginManifest};
use crate::repo::RepoRoot;
use crate::validation::ValidationIssue;

/// Validate marketplace.json against the actual plugin directories.
///
/// Checks that marketplace.json exists and is valid, that each listed plugin
/// source resolves to a real directory with a plugin.json, and that all plugin
/// directories are listed.
pub fn validate_marketplace(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");

    // Check 1: marketplace.json exists
    if !fs.exists(&marketplace_path) {
        issues.push(ValidationIssue::error("marketplace", "marketplace.json is missing"));
        return Ok(issues);
    }

    // Check 2: parses as valid JSON
    let Ok(content) = fs.read_to_string(&marketplace_path) else {
        issues.push(ValidationIssue::error("marketplace", "marketplace.json could not be read"));
        return Ok(issues);
    };
    let marketplace: Marketplace = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            issues.push(ValidationIssue::error(
                "marketplace",
                format!("marketplace.json is malformed: {e}"),
            ));
            return Ok(issues);
        }
    };

    // Check 3: each entry's source resolves to a directory with plugin.json
    for entry in &marketplace.plugins {
        let plugin_path = resolve_source_path(&root.path, &entry.source);

        if !fs.exists(&plugin_path) || !fs.is_dir(&plugin_path) {
            issues.push(ValidationIssue::error(
                "marketplace",
                format!(
                    "plugin \"{}\" source \"{}\" does not resolve to an existing directory",
                    entry.name, entry.source
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

        // Check name match between marketplace entry and plugin.json
        if let Ok(pj_content) = fs.read_to_string(&plugin_json) {
            if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&pj_content) {
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
    }

    // Check 4: unlisted plugins — directories in plugins/ with plugin.json not in marketplace
    let plugins_dir = root.path.join("plugins");
    if fs.is_dir(&plugins_dir) {
        if let Ok(entries) = fs.read_dir(&plugins_dir) {
            let listed_sources: Vec<_> = marketplace
                .plugins
                .iter()
                .map(|e| resolve_source_path(&root.path, &e.source))
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
        }
    }

    Ok(issues)
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

    // Walk plugins/ directory to discover plugins
    let plugins_dir = root.path.join("plugins");
    let mut entries = Vec::new();

    if fs.is_dir(&plugins_dir) {
        if let Ok(dir_entries) = fs.read_dir(&plugins_dir) {
            for dir_entry in dir_entries {
                if !dir_entry.is_dir {
                    continue;
                }
                let plugin_json = dir_entry.path.join(".claude-plugin").join("plugin.json");
                if !fs.exists(&plugin_json) {
                    continue;
                }

                let manifest: PluginManifest = load_json(&plugin_json, fs)?;
                let source = format!("./plugins/{}", dir_entry.file_name);

                // Preserve category from existing entry if the name matches
                let category =
                    existing_by_name.get(manifest.name.as_str()).and_then(|e| e.category.clone());

                entries.push(MarketplaceEntry {
                    name: manifest.name,
                    description: manifest.description.unwrap_or_default(),
                    source,
                    category,
                });
            }
        }
    }

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

/// Resolve a marketplace source path (which may start with `./`) relative to
/// the repository root.
fn resolve_source_path(root: &std::path::Path, source: &str) -> std::path::PathBuf {
    let cleaned = source.strip_prefix("./").unwrap_or(source);
    root.join(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_types::{MarketplaceMetadata, Owner};
    use crate::repo::RepoKind;
    use crate::test_support::{fake_marketplace_json, fake_plugin_json};
    use crate::validation::IssueLevel;
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    fn marketplace_root(fs: &FakeFilesystem, marketplace_json: &str) -> RepoRoot {
        fs.add_file("/repo/.claude-plugin/marketplace.json", marketplace_json);
        fs.add_dir("/repo/plugins");
        RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        }
    }

    // -----------------------------------------------------------------------
    // Validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_marketplace_ok() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("alpha", "Alpha plugin", "./plugins/alpha")]);
        let root = marketplace_root(&fs, &json);

        fs.add_dir("/repo/plugins/alpha");
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));

        let issues = validate_marketplace(&root, &fs).unwrap();
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_marketplace_missing_plugin_dir() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("ghost", "Ghost plugin", "./plugins/ghost")]);
        let root = marketplace_root(&fs, &json);
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
        let root = marketplace_root(&fs, &json);

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
        let root = marketplace_root(&fs, &json);

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
        assert_eq!(mp.plugins[0].source, "./plugins/alpha");
        assert_eq!(mp.plugins[1].name, "beta");
        assert_eq!(mp.plugins[1].source, "./plugins/beta");
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
                description: "A curated marketplace".to_string(),
                version: "2.0.0".to_string(),
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
        assert_eq!(mp.metadata.as_ref().unwrap().version, "2.0.0");
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
                source: "./plugins/alpha".to_string(),
                category: Some("ecosystem".to_string()),
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
                source: "./plugins/existing".to_string(),
                category: Some("tools".to_string()),
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
