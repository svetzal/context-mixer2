use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use cmx::gateway::Filesystem;
use cmx::json_file::{load_json, save_json};
use cmx::scan::scan_source_with;
use cmx::types::ArtifactKind;

use crate::plugin_types::{Author, Marketplace, PluginManifest};
use crate::repo::{RepoKind, RepoRoot};
use crate::validation::ValidationIssue;

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

/// Scan all plugins in a marketplace repo.
///
/// Reads `marketplace.json` for the plugin list, then for each plugin:
/// 1. Loads `plugin.json` for metadata
/// 2. Uses `cmx::scan::scan_source_with()` to discover agents/skills
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

        let scan_result = scan_source_with(&plugin_path, fs)?;

        let mut agents = Vec::new();
        let mut skills = Vec::new();
        for artifact in scan_result.artifacts {
            match artifact.kind {
                ArtifactKind::Agent => agents.push(artifact),
                ArtifactKind::Skill => skills.push(artifact),
            }
        }

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

/// Validate a single plugin at the given path.
///
/// Checks plugin.json presence and validity, agent frontmatter, and skill
/// structure. The `dir_name` is the directory name used for context in issues.
pub fn validate_plugin(
    plugin_path: &Path,
    dir_name: &str,
    fs: &dyn Filesystem,
) -> Result<Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    let manifest_path = plugin_path.join(".claude-plugin").join("plugin.json");

    // Check 1: plugin.json exists
    if !fs.exists(&manifest_path) {
        issues.push(ValidationIssue::error(dir_name, "plugin.json is missing"));
        return Ok(issues);
    }

    // Check 2: plugin.json reads and parses as valid JSON
    let Ok(content) = fs.read_to_string(&manifest_path) else {
        issues.push(ValidationIssue::error(dir_name, "plugin.json could not be read"));
        return Ok(issues);
    };
    let manifest: PluginManifest = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            issues.push(ValidationIssue::error(dir_name, format!("plugin.json is malformed: {e}")));
            return Ok(issues);
        }
    };

    validate_manifest_fields(&manifest, dir_name, &mut issues);

    // Use cmx scan to discover what agents/skills have valid frontmatter
    let scan_result = scan_source_with(plugin_path, fs)?;
    let discovered_agents: Vec<_> = scan_result
        .artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Agent)
        .map(|a| a.name.clone())
        .collect();
    let discovered_skills: Vec<_> = scan_result
        .artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Skill)
        .map(|a| a.name.clone())
        .collect();

    validate_agents_dir(plugin_path, dir_name, &discovered_agents, fs, &mut issues);
    validate_skills_dir(plugin_path, dir_name, &discovered_skills, fs, &mut issues);

    Ok(issues)
}

fn validate_manifest_fields(
    manifest: &PluginManifest,
    dir_name: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if manifest.name.is_empty() {
        issues.push(ValidationIssue::error(dir_name, "plugin.json has empty name field"));
    }

    if !manifest.name.is_empty() && manifest.name != dir_name {
        issues.push(ValidationIssue::warning(
            dir_name,
            format!(
                "plugin.json name \"{}\" does not match directory \"{}\"",
                manifest.name, dir_name
            ),
        ));
    }
}

fn validate_agents_dir(
    plugin_path: &Path,
    dir_name: &str,
    discovered_agents: &[String],
    fs: &dyn Filesystem,
    issues: &mut Vec<ValidationIssue>,
) {
    let agents_dir = plugin_path.join("agents");
    if !fs.is_dir(&agents_dir) {
        return;
    }
    let Ok(entries) = fs.read_dir(&agents_dir) else {
        return;
    };
    for entry in entries {
        let is_md = Path::new(&entry.file_name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
        if entry.is_dir || !is_md {
            continue;
        }
        let agent_name = entry.file_name.trim_end_matches(".md").to_string();
        if !discovered_agents.contains(&agent_name) {
            issues.push(ValidationIssue::warning(
                dir_name,
                format!("agents/{} has no frontmatter name field", entry.file_name),
            ));
        }
    }
}

fn validate_skills_dir(
    plugin_path: &Path,
    dir_name: &str,
    discovered_skills: &[String],
    fs: &dyn Filesystem,
    issues: &mut Vec<ValidationIssue>,
) {
    let skills_dir = plugin_path.join("skills");
    if !fs.is_dir(&skills_dir) {
        return;
    }
    let Ok(entries) = fs.read_dir(&skills_dir) else {
        return;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let skill_md = entry.path.join("SKILL.md");
        if !fs.exists(&skill_md) {
            issues.push(ValidationIssue::warning(
                dir_name,
                format!("skills/{} is missing SKILL.md", entry.file_name),
            ));
        } else if !discovered_skills.contains(&entry.file_name) {
            issues.push(ValidationIssue::warning(
                dir_name,
                format!("skills/{}/SKILL.md has no frontmatter description field", entry.file_name),
            ));
        }
    }
}

/// Validate all plugins listed in the marketplace.
pub fn validate_all_plugins(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");
    let marketplace: Marketplace = load_json(&marketplace_path, fs)?;

    let mut all_issues = Vec::new();
    for entry in &marketplace.plugins {
        let plugin_path = resolve_source_path(&root.path, &entry.source);
        let dir_name = plugin_path
            .file_name()
            .map_or_else(|| entry.name.clone(), |n| n.to_string_lossy().to_string());

        if !fs.exists(&plugin_path) {
            all_issues.push(ValidationIssue::error(
                dir_name,
                format!("source path \"{}\" does not exist", entry.source),
            ));
            continue;
        }

        let mut issues = validate_plugin(&plugin_path, &dir_name, fs)?;
        all_issues.append(&mut issues);
    }

    Ok(all_issues)
}

/// Resolve a marketplace source path (which may start with `./`) relative to
/// the repository root.
fn resolve_source_path(root: &Path, source: &str) -> PathBuf {
    let cleaned = source.strip_prefix("./").unwrap_or(source);
    root.join(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::IssueLevel;
    use cmx::gateway::fakes::FakeFilesystem;

    use crate::test_support::{fake_marketplace_json_with_categories, fake_plugin_json};

    /// Helper to build agent markdown content with valid frontmatter.
    fn agent_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nAgent body.\n")
    }

    /// Helper to build skill SKILL.md content with valid frontmatter.
    fn skill_md(description: &str) -> String {
        format!("---\ndescription: {description}\n---\n# Skill\n\nSkill body.\n")
    }

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

    #[test]
    fn scan_plugins_finds_all() {
        let fs = FakeFilesystem::new();

        let marketplace_json = fake_marketplace_json_with_categories(&[
            ("alpha-ecosystem", "Alpha tools", "./plugins/alpha-ecosystem", Some("ecosystem")),
            ("beta-ecosystem", "Beta tools", "./plugins/beta-ecosystem", Some("ecosystem")),
        ]);
        let root = marketplace_root(&fs, &marketplace_json);

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
        let root = marketplace_root(&fs, &marketplace_json);

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
        let root = marketplace_root(&fs, &marketplace_json);

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

    // -----------------------------------------------------------------------
    // Plugin validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_plugin_ok() {
        let fs = FakeFilesystem::new();
        fs.add_file("/plugin/.claude-plugin/plugin.json", fake_plugin_json("my-plugin"));
        fs.add_file("/plugin/agents/reviewer.md", agent_md("reviewer", "Reviews code"));
        fs.add_file("/plugin/skills/formatter/SKILL.md", skill_md("Formats code"));

        let issues = validate_plugin(Path::new("/plugin"), "my-plugin", &fs).unwrap();
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn validate_plugin_missing_manifest() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/plugin");
        // No plugin.json

        let issues = validate_plugin(Path::new("/plugin"), "my-plugin", &fs).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].level, IssueLevel::Error);
        assert!(
            issues[0].message.contains("missing"),
            "unexpected message: {}",
            issues[0].message
        );
    }

    #[test]
    fn validate_plugin_name_mismatch() {
        let fs = FakeFilesystem::new();
        fs.add_file("/plugin/.claude-plugin/plugin.json", fake_plugin_json("different-name"));

        let issues = validate_plugin(Path::new("/plugin"), "my-plugin", &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("does not match"),
            "unexpected message: {}",
            warnings[0].message
        );
    }

    #[test]
    fn validate_plugin_agent_missing_frontmatter() {
        let fs = FakeFilesystem::new();
        fs.add_file("/plugin/.claude-plugin/plugin.json", fake_plugin_json("my-plugin"));
        // Agent file with no valid frontmatter
        fs.add_file("/plugin/agents/bad-agent.md", "# No frontmatter here\n");

        let issues = validate_plugin(Path::new("/plugin"), "my-plugin", &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("frontmatter"),
            "unexpected message: {}",
            warnings[0].message
        );
    }

    #[test]
    fn validate_plugin_skill_missing_skill_md() {
        let fs = FakeFilesystem::new();
        fs.add_file("/plugin/.claude-plugin/plugin.json", fake_plugin_json("my-plugin"));
        // Skill directory without SKILL.md — register parent too for read_dir
        fs.add_dir("/plugin/skills");
        fs.add_dir("/plugin/skills/broken-skill");

        let issues = validate_plugin(Path::new("/plugin"), "my-plugin", &fs).unwrap();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].message.contains("missing SKILL.md"),
            "unexpected message: {}",
            warnings[0].message
        );
    }
}
