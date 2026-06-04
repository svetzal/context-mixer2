use std::path::Path;

use anyhow::Result;
use cmx::gateway::Filesystem;
use cmx::scan::scan_source;
use cmx::types::ArtifactKind;

use crate::plugin_types::{Marketplace, PluginManifest};
use crate::repo::{RepoRoot, resolve_source_path};
use crate::validation::{ValidationIssue, load_and_validate_json};

/// Validate a single plugin at the given path.
///
/// Checks plugin.json presence and validity, agent frontmatter, and skill
/// structure. The `dir_name` is the directory name used for context in issues.
pub fn validate_plugin(
    plugin_path: &Path,
    dir_name: &str,
    fs: &dyn Filesystem,
) -> Result<Vec<ValidationIssue>> {
    let manifest_path = plugin_path.join(".claude-plugin").join("plugin.json");

    let (maybe_manifest, early_issues) =
        load_and_validate_json::<PluginManifest>(&manifest_path, dir_name, "plugin.json", fs)?;
    if !early_issues.is_empty() {
        return Ok(early_issues);
    }
    let manifest = maybe_manifest.unwrap();

    let mut issues = Vec::new();

    validate_manifest_fields(&manifest, dir_name, &mut issues);

    // Use cmx scan to discover what agents/skills have valid frontmatter
    let scan_result = scan_source(plugin_path, fs)?;
    let (discovered_agents, discovered_skills) = artifact_names_by_kind(&scan_result.artifacts);

    validate_artifact_dir(
        ArtifactKind::Agent,
        plugin_path,
        dir_name,
        &discovered_agents,
        fs,
        &mut issues,
    );
    validate_artifact_dir(
        ArtifactKind::Skill,
        plugin_path,
        dir_name,
        &discovered_skills,
        fs,
        &mut issues,
    );

    Ok(issues)
}

/// Validate all plugins listed in the marketplace.
pub fn validate_all_plugins(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    use cmx::json_file::load_json;
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

/// Extract artifact names grouped by kind from a slice of artifacts.
///
/// Returns `(agent_names, skill_names)`.
fn artifact_names_by_kind(artifacts: &[cmx::types::Artifact]) -> (Vec<String>, Vec<String>) {
    let agents = artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Agent)
        .map(|a| a.name.clone())
        .collect();
    let skills = artifacts
        .iter()
        .filter(|a| a.kind == ArtifactKind::Skill)
        .map(|a| a.name.clone())
        .collect();
    (agents, skills)
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

fn validate_artifact_dir(
    kind: ArtifactKind,
    plugin_path: &Path,
    dir_name: &str,
    discovered_names: &[String],
    fs: &dyn Filesystem,
    issues: &mut Vec<ValidationIssue>,
) {
    let artifact_dir = plugin_path.join(kind.subdir_name());
    if !fs.is_dir(&artifact_dir) {
        return;
    }
    let entries = match fs.read_dir(&artifact_dir) {
        Ok(e) => e,
        Err(e) => {
            issues.push(ValidationIssue::error(
                format!("plugin/{dir_name}"),
                format!("could not read {} directory: {e}", kind.subdir_name()),
            ));
            return;
        }
    };
    for entry in entries {
        match kind {
            ArtifactKind::Agent => validate_agent_entry(&entry, discovered_names, dir_name, issues),
            ArtifactKind::Skill => {
                validate_skill_entry(&entry, discovered_names, dir_name, fs, issues);
            }
        }
    }
}

/// Pure predicate: determine the validation issue message for an agent entry,
/// if any. Returns `None` when the entry is valid or not an agent file.
/// The shell computes `discovered_names` from the scan result and passes it in.
fn agent_entry_issue(
    entry: &cmx::gateway::DirEntry,
    discovered_names: &[String],
) -> Option<String> {
    let is_md = Path::new(&entry.file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    if entry.is_dir || !is_md {
        return None;
    }
    let artifact_name = ArtifactKind::Agent
        .artifact_name_from_path(Path::new(&entry.file_name))
        .unwrap_or_default();
    if discovered_names.contains(&artifact_name) {
        None
    } else {
        Some(format!("agents/{} has no frontmatter name field", entry.file_name))
    }
}

/// Pure predicate: determine the validation issue message for a skill entry,
/// if any. `skill_md_exists` is computed by the shell via `fs.exists`.
fn skill_entry_issue(
    entry: &cmx::gateway::DirEntry,
    skill_md_exists: bool,
    discovered_names: &[String],
) -> Option<String> {
    if !entry.is_dir {
        return None;
    }
    if !skill_md_exists {
        Some(format!("skills/{} is missing SKILL.md", entry.file_name))
    } else if !discovered_names.contains(&entry.file_name) {
        Some(format!(
            "skills/{}/SKILL.md has no frontmatter description field",
            entry.file_name
        ))
    } else {
        None
    }
}

fn validate_agent_entry(
    entry: &cmx::gateway::DirEntry,
    discovered_names: &[String],
    dir_name: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(msg) = agent_entry_issue(entry, discovered_names) {
        issues.push(ValidationIssue::warning(dir_name, msg));
    }
}

fn validate_skill_entry(
    entry: &cmx::gateway::DirEntry,
    discovered_names: &[String],
    dir_name: &str,
    fs: &dyn Filesystem,
    issues: &mut Vec<ValidationIssue>,
) {
    let content_path = ArtifactKind::Skill.content_path(&entry.path);
    let skill_md_exists = fs.exists(&content_path);
    if let Some(msg) = skill_entry_issue(entry, skill_md_exists, discovered_names) {
        issues.push(ValidationIssue::warning(dir_name, msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::IssueLevel;
    use cmx::gateway::DirEntry;
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    use crate::test_support::fake_plugin_json;

    fn file_entry(file_name: &str, path: &str) -> DirEntry {
        DirEntry {
            path: PathBuf::from(path),
            file_name: file_name.to_string(),
            is_dir: false,
        }
    }

    fn dir_entry(file_name: &str, path: &str) -> DirEntry {
        DirEntry {
            path: PathBuf::from(path),
            file_name: file_name.to_string(),
            is_dir: true,
        }
    }

    // --- agent_entry_issue (pure, no FakeFilesystem) ---

    #[test]
    fn agent_entry_issue_no_issue_for_discovered_agent() {
        let entry = file_entry("reviewer.md", "/plugin/agents/reviewer.md");
        let result = agent_entry_issue(&entry, &["reviewer".to_string()]);
        assert!(result.is_none());
    }

    #[test]
    fn agent_entry_issue_warns_when_missing_frontmatter() {
        let entry = file_entry("bad-agent.md", "/plugin/agents/bad-agent.md");
        let result = agent_entry_issue(&entry, &[]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("frontmatter"));
    }

    #[test]
    fn agent_entry_issue_skips_directories() {
        let entry = dir_entry("subdir", "/plugin/agents/subdir");
        assert!(agent_entry_issue(&entry, &[]).is_none());
    }

    #[test]
    fn agent_entry_issue_skips_non_md_files() {
        let entry = file_entry("script.py", "/plugin/agents/script.py");
        assert!(agent_entry_issue(&entry, &[]).is_none());
    }

    // --- skill_entry_issue (pure, no FakeFilesystem) ---

    #[test]
    fn skill_entry_issue_no_issue_for_valid_skill() {
        let entry = dir_entry("formatter", "/plugin/skills/formatter");
        let result = skill_entry_issue(&entry, true, &["formatter".to_string()]);
        assert!(result.is_none());
    }

    #[test]
    fn skill_entry_issue_missing_skill_md() {
        let entry = dir_entry("broken", "/plugin/skills/broken");
        let result = skill_entry_issue(&entry, false, &[]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("missing SKILL.md"));
    }

    #[test]
    fn skill_entry_issue_skill_md_present_but_no_frontmatter() {
        let entry = dir_entry("badfm", "/plugin/skills/badfm");
        let result = skill_entry_issue(&entry, true, &[]);
        assert!(result.is_some());
        assert!(result.unwrap().contains("frontmatter description"));
    }

    #[test]
    fn skill_entry_issue_skips_non_directories() {
        let entry = file_entry("SKILL.md", "/plugin/skills/SKILL.md");
        assert!(skill_entry_issue(&entry, false, &[]).is_none());
    }

    /// Helper to build agent markdown content with valid frontmatter.
    fn agent_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nAgent body.\n")
    }

    /// Helper to build skill SKILL.md content with valid frontmatter.
    fn skill_md(description: &str) -> String {
        format!("---\ndescription: {description}\n---\n# Skill\n\nSkill body.\n")
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
