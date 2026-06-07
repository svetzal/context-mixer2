use anyhow::Result;
use std::path::Path;

use crate::gateway::filesystem::Filesystem;
use crate::scan_marketplace;
use crate::types::{Artifact, ArtifactKind};

mod frontmatter;
pub(crate) use frontmatter::{
    Frontmatter, artifact_from_frontmatter, parse_agent_frontmatter_str, parse_frontmatter_str,
};
pub use frontmatter::{
    extract_field, extract_metadata_field, extract_version, extract_version_from_content,
    split_frontmatter_and_body,
};

// ---------------------------------------------------------------------------
// Warning types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanWarning {
    pub message: String,
}

#[derive(Debug)]
pub struct ScanResult {
    pub artifacts: Vec<Artifact>,
    pub warnings: Vec<ScanWarning>,
}

// ---------------------------------------------------------------------------
// Testable variant (accepts injected Filesystem)
// ---------------------------------------------------------------------------

pub fn scan_source(root: &Path, fs: &dyn Filesystem) -> Result<ScanResult> {
    let marketplace = root.join(".claude-plugin").join("marketplace.json");
    let mut warnings = Vec::new();

    let mut artifacts = if fs.exists(&marketplace) {
        scan_marketplace::scan_marketplace_with(root, &marketplace, fs, &mut warnings)?
    } else {
        let mut arts = Vec::new();
        walk_dir_with(root, &mut arts, fs)?;
        arts
    };

    artifacts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ScanResult {
        artifacts,
        warnings,
    })
}

pub(crate) fn try_parse_artifact(
    kind: ArtifactKind,
    path: &Path,
    fs: &dyn Filesystem,
) -> Option<Artifact> {
    let content_path = kind.content_path(path);
    let content = fs.read_to_string(&content_path).ok()?;
    let fm = kind.parse_frontmatter(&content)?;
    let name = kind.artifact_name_from_path(path)?;
    Some(artifact_from_frontmatter(kind, name, path.to_path_buf(), fm))
}

/// Pure classification of a directory entry before any I/O is performed.
///
/// `Recurse` is returned for non-hidden directories: the walker will first
/// attempt to parse the dir as a skill (requires reading `SKILL.md`) and, if
/// that fails, recurse into it. The pure classifier only sees what the
/// `DirEntry` metadata can tell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryAction {
    /// Parse the file as an agent `.md`.
    ParseAgent,
    /// Non-hidden directory: walker tries skill parse first, then recurses.
    Recurse,
    /// Skip this entry entirely.
    Skip,
}

/// Classify a single directory entry without performing any I/O.
///
/// Rules encoded here (from `walk_dir_with`):
/// - Hidden entries (name starts with `.`) → `Skip`
/// - Directories → `Recurse` (walker tries skill parse first, then recurses)
/// - `.md` files (not `SKILL.md`) directly inside an `agents/` directory → `ParseAgent`
/// - Everything else → `Skip`
pub(crate) fn classify_entry(entry: &crate::gateway::DirEntry, parent_dir: &Path) -> EntryAction {
    if entry.file_name.starts_with('.') {
        return EntryAction::Skip;
    }
    if entry.is_dir {
        return EntryAction::Recurse;
    }
    if entry.path.extension().is_some_and(|ext| ext == "md")
        && entry.file_name != "SKILL.md"
        && parent_dir.file_name().is_some_and(|d| d == "agents")
    {
        return EntryAction::ParseAgent;
    }
    EntryAction::Skip
}

pub(crate) fn walk_dir_with(
    dir: &Path,
    artifacts: &mut Vec<Artifact>,
    fs: &dyn Filesystem,
) -> Result<()> {
    let Ok(entries) = fs.read_dir(dir) else {
        return Ok(());
    };

    for entry in entries {
        match classify_entry(&entry, dir) {
            EntryAction::Recurse => {
                if let Some(artifact) = try_parse_artifact(ArtifactKind::Skill, &entry.path, fs) {
                    artifacts.push(artifact);
                    // Don't recurse into skill directories — .md files inside
                    // are reference material, not agents
                } else {
                    walk_dir_with(&entry.path, artifacts, fs)?;
                }
            }
            EntryAction::ParseAgent => {
                if let Some(artifact) = try_parse_artifact(ArtifactKind::Agent, &entry.path, fs) {
                    artifacts.push(artifact);
                }
            }
            EntryAction::Skip => {}
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// ArtifactKind frontmatter parsing (scan-local, depends on local parsers)
// ---------------------------------------------------------------------------

impl ArtifactKind {
    /// Parse frontmatter from content using the appropriate strategy for this
    /// artifact kind.  Agents require `name` and `description` fields; skills
    /// only require the standard frontmatter block.
    pub(crate) fn parse_frontmatter(self, content: &str) -> Option<Frontmatter> {
        match self {
            ArtifactKind::Agent => parse_agent_frontmatter_str(content),
            ArtifactKind::Skill => parse_frontmatter_str(content),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::DirEntry;
    use crate::gateway::fakes::FakeFilesystem;
    use crate::test_support::{
        agent_content, metadata_versioned_agent_content, metadata_versioned_skill_content,
        skill_content,
    };
    use std::path::PathBuf;

    fn make_entry(path: &str, file_name: &str, is_dir: bool) -> DirEntry {
        DirEntry {
            path: PathBuf::from(path),
            file_name: file_name.to_string(),
            is_dir,
        }
    }

    // --- classify_entry (pure, no FakeFilesystem) ---

    #[test]
    fn classify_entry_hidden_dir_is_skip() {
        let entry = make_entry("/repo/.hidden", ".hidden", true);
        assert_eq!(classify_entry(&entry, Path::new("/repo")), EntryAction::Skip);
    }

    #[test]
    fn classify_entry_plain_subdir_is_recurse() {
        let entry = make_entry("/repo/subdir", "subdir", true);
        assert_eq!(classify_entry(&entry, Path::new("/repo")), EntryAction::Recurse);
    }

    #[test]
    fn classify_entry_md_in_agents_is_parse_agent() {
        let entry = make_entry("/repo/agents/my-agent.md", "my-agent.md", false);
        assert_eq!(classify_entry(&entry, Path::new("/repo/agents")), EntryAction::ParseAgent);
    }

    #[test]
    fn classify_entry_skill_md_is_not_parse_agent() {
        let entry = make_entry("/repo/my-skill/SKILL.md", "SKILL.md", false);
        assert_eq!(classify_entry(&entry, Path::new("/repo/my-skill")), EntryAction::Skip);
    }

    #[test]
    fn classify_entry_md_outside_agents_dir_is_skip() {
        let entry = make_entry("/repo/docs/readme.md", "readme.md", false);
        assert_eq!(classify_entry(&entry, Path::new("/repo/docs")), EntryAction::Skip);
    }

    #[test]
    fn classify_entry_non_md_file_is_skip() {
        let entry = make_entry("/repo/agents/script.py", "script.py", false);
        assert_eq!(classify_entry(&entry, Path::new("/repo/agents")), EntryAction::Skip);
    }

    // ---------------------------------------------------------------------------
    // scan_source_with tests using FakeFilesystem
    // ---------------------------------------------------------------------------

    #[test]
    fn scan_empty_directory_returns_empty() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn scan_ignores_md_file_without_any_frontmatter() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/plain.md", "# No frontmatter here");
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn scan_ignores_md_file_without_agent_frontmatter() {
        let fs = FakeFilesystem::new();
        // Has frontmatter but no 'name:' field — not an agent
        fs.add_file("/repo/agents/not-agent.md", "---\ndescription: only desc\n---\n");
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn scan_ignores_agent_frontmatter_outside_agents_dir() {
        let fs = FakeFilesystem::new();
        // Valid agent frontmatter but not in an agents/ directory
        fs.add_file("/repo/docs/my-agent.md", agent_content("my-agent", "Does things"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn scan_finds_agent_with_valid_frontmatter() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/agents/my-agent.md", agent_content("my-agent", "Does things"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "my-agent");
        assert_eq!(result.artifacts[0].kind, ArtifactKind::Agent);
        assert_eq!(result.artifacts[0].description, "Does things");
        assert_eq!(result.artifacts[0].path, PathBuf::from("/repo/agents/my-agent.md"));
    }

    #[test]
    fn scan_finds_agent_with_metadata_version() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/agents/my-agent.md",
            metadata_versioned_agent_content("my-agent", "Does things", "1.3.2"),
        );
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].version.as_deref(), Some("1.3.2"));
    }

    #[test]
    fn scan_finds_skill_with_metadata_version() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/my-skill/SKILL.md",
            metadata_versioned_skill_content("A skill", "2.1.0"),
        );
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn scan_skips_hidden_directories() {
        let fs = FakeFilesystem::new();
        // File inside a hidden dir — should be ignored even if parent is named agents
        fs.add_file("/repo/.hidden/agents/secret.md", agent_content("secret", "Hidden"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
    }

    #[test]
    fn scan_finds_multiple_agents_sorted_by_name() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/agents/zebra.md", agent_content("zebra", "Z agent"));
        fs.add_file("/repo/agents/alpha.md", agent_content("alpha", "A agent"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 2);
        assert_eq!(result.artifacts[0].name, "alpha");
        assert_eq!(result.artifacts[1].name, "zebra");
    }

    #[test]
    fn scan_finds_skill_with_skill_md() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/my-skill/SKILL.md", skill_content("A skill"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "my-skill");
        assert_eq!(result.artifacts[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn scan_does_not_recurse_into_skill_directories() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/my-skill/SKILL.md", skill_content("A skill"));
        // Reference .md files inside the skill dir should NOT be detected as agents
        fs.add_file(
            "/repo/my-skill/references/some-feature.md",
            agent_content("some-feature", "A reference doc"),
        );
        fs.add_file(
            "/repo/my-skill/references/another-feature.md",
            agent_content("another-feature", "Another reference doc"),
        );
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "my-skill");
        assert_eq!(result.artifacts[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn scan_finds_both_agents_and_skills() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/agents/alpha.md", agent_content("alpha", "An agent"));
        fs.add_file("/repo/my-skill/SKILL.md", skill_content("A skill"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 2);
        let kinds: Vec<_> = result.artifacts.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&ArtifactKind::Agent));
        assert!(kinds.contains(&ArtifactKind::Skill));
    }

    // ---------------------------------------------------------------------------
    // Marketplace: source-path fallback (no explicit agents/skills arrays)
    // ---------------------------------------------------------------------------

    fn marketplace_json(plugins_json: &str) -> String {
        format!(
            r#"{{
  "name": "test-marketplace",
  "owner": {{ "name": "Test" }},
  "plugins": [{plugins_json}]
}}"#
        )
    }

    #[test]
    fn marketplace_source_path_walks_directory_for_agents() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "my-plugin", "source": "./plugins/my-plugin" }"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/agents/reviewer.md",
            agent_content("reviewer", "Reviews code"),
        );
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "reviewer");
        assert_eq!(result.artifacts[0].kind, ArtifactKind::Agent);
    }

    #[test]
    fn marketplace_source_path_walks_directory_for_skills() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "my-plugin", "source": "./plugins/my-plugin" }"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/my-skill/SKILL.md",
            skill_content("A discovered skill"),
        );
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "my-skill");
        assert_eq!(result.artifacts[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn marketplace_source_path_finds_mixed_artifacts() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "my-plugin", "source": "./plugins/my-plugin" }"#),
        );
        fs.add_file(
            "/repo/plugins/my-plugin/agents/checker.md",
            agent_content("checker", "Checks things"),
        );
        fs.add_file("/repo/plugins/my-plugin/pdf/SKILL.md", skill_content("PDF processing"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 2);
        let names: Vec<_> = result.artifacts.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"checker"));
        assert!(names.contains(&"pdf"));
    }

    #[test]
    fn marketplace_explicit_arrays_take_precedence_over_source_walk() {
        let fs = FakeFilesystem::new();
        // Plugin has both source AND explicit skills array — should use explicit only
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{
                    "name": "doc-skills",
                    "source": "./",
                    "strict": false,
                    "skills": ["./skills/pdf"]
                }"#,
            ),
        );
        fs.add_file("/repo/skills/pdf/SKILL.md", skill_content("PDF skill"));
        // This agent exists in the repo but isn't in the explicit arrays
        fs.add_file("/repo/agents/extra-agent.md", agent_content("extra", "Should not appear"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(result.artifacts[0].name, "pdf");
        assert_eq!(result.artifacts[0].kind, ArtifactKind::Skill);
    }

    #[test]
    fn marketplace_multiple_source_plugins_all_walked() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{ "name": "plugin-a", "source": "./plugins/a" },
                   { "name": "plugin-b", "source": "./plugins/b" }"#,
            ),
        );
        fs.add_file("/repo/plugins/a/agents/agent-a.md", agent_content("agent-a", "From plugin A"));
        fs.add_file("/repo/plugins/b/agents/agent-b.md", agent_content("agent-b", "From plugin B"));
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert_eq!(result.artifacts.len(), 2);
        let names: Vec<_> = result.artifacts.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"agent-a"));
        assert!(names.contains(&"agent-b"));
    }

    #[test]
    fn marketplace_missing_source_path_warns_and_continues() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(r#"{ "name": "ghost", "source": "./nonexistent" }"#),
        );
        // Should not error, just warn and return a warning
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].message.contains("does not exist"),
            "unexpected warning: {}",
            result.warnings[0].message
        );
    }

    #[test]
    fn marketplace_object_source_warns_and_continues() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{ "name": "remote-plugin", "source": { "source": "url", "url": "https://github.com/example/plugin.git" } }"#,
            ),
        );
        // Remote sources are not supported — should warn and return empty
        let result = scan_source(Path::new("/repo"), &fs).unwrap();
        assert!(result.artifacts.is_empty());
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].message.contains("remote source type"),
            "unexpected warning: {}",
            result.warnings[0].message
        );
    }
}
