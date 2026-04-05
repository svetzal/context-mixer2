use anyhow::Result;
use cmx::gateway::Filesystem;

use crate::facet::validate_facets;
use crate::marketplace::validate_marketplace;
use crate::plugin::validate_all_plugins;
use crate::repo::RepoRoot;
use crate::validation::ValidationIssue;

/// Run all validation checks: marketplace, plugin, and facet validation.
pub fn validate_all(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<ValidationIssue>> {
    let mut issues = validate_marketplace(root, fs)?;
    let mut plugin_issues = validate_all_plugins(root, fs)?;
    issues.append(&mut plugin_issues);
    let mut facet_issues = validate_facets(root, fs)?;
    issues.append(&mut facet_issues);
    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoKind;
    use crate::test_support::{fake_marketplace_json, fake_plugin_json};
    use crate::validation::IssueLevel;
    use cmx::gateway::fakes::FakeFilesystem;
    use std::path::PathBuf;

    #[test]
    fn validate_all_combines_results() {
        let fs = FakeFilesystem::new();

        // Marketplace references a plugin whose source doesn't exist (marketplace error)
        let json = fake_marketplace_json(&[
            ("good", "Good plugin", "./plugins/good"),
            ("ghost", "Ghost plugin", "./plugins/ghost"),
        ]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", json.as_str());
        fs.add_dir("/repo/plugins");

        // "good" exists but has a name mismatch in plugin.json (plugin warning)
        fs.add_dir("/repo/plugins/good");
        fs.add_file(
            "/repo/plugins/good/.claude-plugin/plugin.json",
            fake_plugin_json("mismatched-name"),
        );

        // "ghost" doesn't exist at all (marketplace error + plugin error)

        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_facets: false,
            has_plugins_dir: true,
        };

        let issues = validate_all(&root, &fs).unwrap();

        let errors: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Error).collect();
        let warnings: Vec<_> = issues.iter().filter(|i| i.level == IssueLevel::Warning).collect();

        // Should have at least one error (ghost missing dir) and warnings (name mismatch)
        assert!(!errors.is_empty(), "expected at least one error, got none");
        assert!(!warnings.is_empty(), "expected at least one warning, got none");
    }
}
