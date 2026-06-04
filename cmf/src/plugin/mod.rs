use std::path::PathBuf;

use anyhow::{Result, bail};
use cmx::gateway::Filesystem;
use cmx::json_file::{load_json, save_json};
use cmx::scan::scan_source;
use cmx::types::ArtifactKind;

use crate::plugin_types::{Author, Marketplace, PluginManifest};
use crate::repo::{RepoKind, RepoRoot, resolve_source_path};

mod validate;
pub use validate::{validate_all_plugins, validate_plugin};

/// Split a flat list of artifacts into `(agents, skills)`.
fn partition_artifacts(
    artifacts: Vec<cmx::types::Artifact>,
) -> (Vec<cmx::types::Artifact>, Vec<cmx::types::Artifact>) {
    let mut agents = Vec::new();
    let mut skills = Vec::new();
    for artifact in artifacts {
        match artifact.kind {
            ArtifactKind::Agent => agents.push(artifact),
            ArtifactKind::Skill => skills.push(artifact),
        }
    }
    (agents, skills)
}

#[derive(Debug)]
pub struct PluginInfo {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub path: PathBuf,
    pub agents: Vec<cmx::types::Artifact>,
    pub skills: Vec<cmx::types::Artifact>,
}

pub struct PluginList(pub Vec<PluginInfo>);

/// Scan all plugins in a marketplace repo.
///
/// Reads `marketplace.json` for the plugin list, then for each plugin:
/// 1. Loads `plugin.json` for metadata
/// 2. Uses `cmx::scan::scan_source()` to discover agents/skills
pub fn scan_plugins(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<PluginInfo>> {
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");
    let marketplace: Marketplace = load_json(&marketplace_path, fs)?;

    let mut plugins = Vec::new();

    for entry in &marketplace.plugins {
        let plugin_path = resolve_source_path(&root.path, &entry.source);

        if !fs.exists(&plugin_path) {
            eprintln!(
                "warning: plugin '{}' source path '{}' does not exist, skipping",
                entry.name, entry.source
            );
            continue;
        }

        let manifest_path = plugin_path.join(".claude-plugin").join("plugin.json");
        let manifest: PluginManifest = load_json(&manifest_path, fs)?;

        let scan_result = scan_source(&plugin_path, fs)?;
        let (agents, skills) = partition_artifacts(scan_result.artifacts);

        plugins.push(PluginInfo {
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            category: entry.category.clone(),
            path: plugin_path,
            agents,
            skills,
        });
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

/// Scaffold a new plugin directory with `plugin.json` and empty `agents/`
/// and `skills/` directories.
pub fn init_plugin(root: &RepoRoot, name: &str, fs: &dyn Filesystem) -> Result<PathBuf> {
    if root.kind != RepoKind::Marketplace {
        bail!(
            "plugin init requires a marketplace repository (has .claude-plugin/marketplace.json)"
        );
    }

    let plugin_path = root.path.join("plugins").join(name);
    if fs.exists(&plugin_path) {
        bail!("plugin '{}' already exists at {}", name, plugin_path.display());
    }

    let claude_plugin_dir = plugin_path.join(".claude-plugin");
    let agents_dir = plugin_path.join("agents");
    let skills_dir = plugin_path.join("skills");

    fs.create_dir_all(&claude_plugin_dir)?;
    fs.create_dir_all(&agents_dir)?;
    fs.create_dir_all(&skills_dir)?;

    let manifest = PluginManifest {
        name: name.to_string(),
        version: Some("0.1.0".to_string()),
        description: Some(String::new()),
        author: Some(Author {
            name: String::new(),
            email: String::new(),
        }),
        license: Some("MIT".to_string()),
        keywords: Vec::new(),
    };

    let manifest_path = claude_plugin_dir.join("plugin.json");
    save_json(&manifest, &manifest_path, fs)?;

    Ok(plugin_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx::gateway::fakes::FakeFilesystem;

    use crate::test_support::{
        fake_marketplace_json_with_categories, fake_marketplace_root, fake_plugin_json,
    };

    fn make_plugin(
        name: &str,
        version: Option<&str>,
        category: Option<&str>,
        agents: usize,
        skills: usize,
    ) -> PluginInfo {
        PluginInfo {
            name: name.to_string(),
            version: version.map(str::to_string),
            description: None,
            category: category.map(str::to_string),
            path: PathBuf::from("/tmp"),
            agents: (0..agents)
                .map(|_| cmx::types::Artifact {
                    kind: cmx::types::ArtifactKind::Agent,
                    name: "test".to_string(),
                    description: String::new(),
                    path: PathBuf::from("/tmp/test"),
                    version: None,
                    deprecation: None,
                })
                .collect(),
            skills: (0..skills)
                .map(|_| cmx::types::Artifact {
                    kind: cmx::types::ArtifactKind::Skill,
                    name: "test".to_string(),
                    description: String::new(),
                    path: PathBuf::from("/tmp/test"),
                    version: None,
                    deprecation: None,
                })
                .collect(),
        }
    }

    // --- Display for PluginList ---

    #[test]
    fn plugin_list_display_empty() {
        let out = PluginList(vec![]).to_string();
        assert_eq!(out, "Plugins (0):\n");
    }

    #[test]
    fn plugin_list_display_single_plugin() {
        let plugins = vec![make_plugin("my-plugin", Some("1.0.0"), Some("tools"), 1, 2)];
        let out = PluginList(plugins).to_string();
        assert!(out.starts_with("Plugins (1):"));
        assert!(out.contains("my-plugin"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("tools"));
        assert!(out.contains("1 agent "));
        assert!(out.contains("2 skills"));
    }

    #[test]
    fn plugin_list_display_missing_optional_fields() {
        let plugins = vec![make_plugin("my-plugin", None, None, 0, 0)];
        let out = PluginList(plugins).to_string();
        assert!(out.contains("my-plugin"));
        assert!(out.contains('-'));
    }

    /// Helper to build agent markdown content with valid frontmatter.
    fn agent_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nAgent body.\n")
    }

    /// Helper to build skill SKILL.md content with valid frontmatter.
    fn skill_md(description: &str) -> String {
        format!("---\ndescription: {description}\n---\n# Skill\n\nSkill body.\n")
    }

    #[test]
    fn scan_plugins_finds_all() {
        let fs = FakeFilesystem::new();

        let marketplace_json = fake_marketplace_json_with_categories(&[
            ("alpha-ecosystem", "Alpha tools", "./plugins/alpha-ecosystem", Some("ecosystem")),
            ("beta-ecosystem", "Beta tools", "./plugins/beta-ecosystem", Some("ecosystem")),
        ]);
        let root = fake_marketplace_root(&fs, &marketplace_json);

        // alpha has 1 agent
        fs.add_file(
            "/repo/plugins/alpha-ecosystem/.claude-plugin/plugin.json",
            fake_plugin_json("alpha-ecosystem"),
        );
        fs.add_file(
            "/repo/plugins/alpha-ecosystem/agents/reviewer.md",
            agent_md("reviewer", "Reviews code"),
        );

        // beta has 1 skill
        fs.add_file(
            "/repo/plugins/beta-ecosystem/.claude-plugin/plugin.json",
            fake_plugin_json("beta-ecosystem"),
        );
        fs.add_file(
            "/repo/plugins/beta-ecosystem/skills/formatter/SKILL.md",
            skill_md("Formats code"),
        );

        let plugins = scan_plugins(&root, &fs).unwrap();
        assert_eq!(plugins.len(), 2);

        // Sorted by name
        assert_eq!(plugins[0].name, "alpha-ecosystem");
        assert_eq!(plugins[0].agents.len(), 1);
        assert_eq!(plugins[0].skills.len(), 0);
        assert_eq!(plugins[0].category.as_deref(), Some("ecosystem"));

        assert_eq!(plugins[1].name, "beta-ecosystem");
        assert_eq!(plugins[1].agents.len(), 0);
        assert_eq!(plugins[1].skills.len(), 1);
        assert_eq!(plugins[1].category.as_deref(), Some("ecosystem"));
    }

    #[test]
    fn scan_plugins_empty_marketplace() {
        let fs = FakeFilesystem::new();
        let marketplace_json = fake_marketplace_json_with_categories(&[]);
        let root = fake_marketplace_root(&fs, &marketplace_json);

        let plugins = scan_plugins(&root, &fs).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    fn scan_plugins_missing_plugin_dir_skips_with_warning() {
        let fs = FakeFilesystem::new();

        let marketplace_json = fake_marketplace_json_with_categories(&[
            ("exists", "Exists", "./plugins/exists", None),
            ("ghost", "Ghost", "./plugins/ghost", None),
        ]);
        let root = fake_marketplace_root(&fs, &marketplace_json);

        // Only "exists" has a real directory
        fs.add_file("/repo/plugins/exists/.claude-plugin/plugin.json", fake_plugin_json("exists"));
        fs.add_dir("/repo/plugins/exists");

        // "ghost" has no directory at all

        let plugins = scan_plugins(&root, &fs).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "exists");
    }

    #[test]
    fn init_plugin_creates_structure() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", "{}");
        fs.add_dir("/repo/plugins");

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        let result = init_plugin(&root, "my-new-plugin", &fs).unwrap();
        assert_eq!(result, PathBuf::from("/repo/plugins/my-new-plugin"));

        // Verify directories exist
        assert!(fs.is_dir(&PathBuf::from("/repo/plugins/my-new-plugin/.claude-plugin")));
        assert!(fs.is_dir(&PathBuf::from("/repo/plugins/my-new-plugin/agents")));
        assert!(fs.is_dir(&PathBuf::from("/repo/plugins/my-new-plugin/skills")));

        // Verify plugin.json was written and is valid
        let manifest_path = PathBuf::from("/repo/plugins/my-new-plugin/.claude-plugin/plugin.json");
        assert!(fs.exists(&manifest_path));

        let content = fs.read_to_string(&manifest_path).unwrap();
        let manifest: PluginManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(manifest.name, "my-new-plugin");
        assert_eq!(manifest.version.as_deref(), Some("0.1.0"));
        assert_eq!(manifest.license.as_deref(), Some("MIT"));
    }

    #[test]
    fn init_plugin_rejects_duplicate() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", "{}");
        fs.add_dir("/repo/plugins/existing-plugin");

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        let result = init_plugin(&root, "existing-plugin", &fs);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("already exists"),
            "expected 'already exists' in error: {err_msg}"
        );
    }

    #[test]
    fn init_plugin_requires_marketplace_repo() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Unknown,
            has_facets: false,
            has_plugins_dir: false,
        };

        let result = init_plugin(&root, "my-plugin", &fs);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("marketplace"), "expected 'marketplace' in error: {err_msg}");
    }
}
