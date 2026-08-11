//! Output formatting for the `status` command, a submodule of
//! `cmf/src/display/mod.rs`.

use std::collections::BTreeMap;

use cmx::gateway::Filesystem;
use cmx::json_file::load_json;

use crate::intent::scan_intents;
use crate::plugin::scan_plugins;
use crate::plugin_types::Marketplace;
use crate::repo::{RepoKind, RepoRoot};
use crate::validate::validate_all;
use crate::validation::IssueLevel;

/// Render the full `cmf status` report: repo identity, plugin summary,
/// intent summary, and validation summary, in that order.
pub fn status_report(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let mut out = String::new();
    out.push_str(&repo_identity_str(root, fs));
    out.push_str(&plugin_summary_str(root, fs));
    out.push_str(&intent_summary_str(root, fs));
    out.push_str(&validation_summary_str(root, fs));
    out
}

fn repo_identity_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let marketplace_path = root.path.join(".claude-plugin").join("marketplace.json");
    let name = load_json::<Marketplace>(&marketplace_path, fs)
        .ok()
        .map(|m| m.name)
        .filter(|n| !n.is_empty());

    let kind_label = match root.kind {
        RepoKind::Marketplace => "marketplace",
        RepoKind::Plugin => "plugin",
        RepoKind::IntentsOnly => "intents-only",
        RepoKind::Unknown => "unknown",
    };

    let mut out = match name {
        Some(n) => format!("Repository: {n} ({kind_label})\n"),
        None => format!("Repository: ({kind_label})\n"),
    };
    let _ = std::fmt::write(&mut out, format_args!("Root: {}\n", root.path.display()));
    out
}

fn plugin_summary_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let Ok(plugins) = scan_plugins(root, fs) else {
        return String::new();
    };

    if plugins.is_empty() {
        return String::new();
    }

    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_agents = 0usize;
    let mut total_skills = 0usize;

    for plugin in &plugins {
        let cat = plugin.category.as_deref().unwrap_or("uncategorized").to_string();
        *by_category.entry(cat).or_default() += 1;
        total_agents += plugin.agents.len();
        total_skills += plugin.skills.len();
    }

    let breakdown: Vec<String> =
        by_category.iter().map(|(cat, count)| format!("{count} {cat}")).collect();

    let plugin_count = plugins.len();
    let breakdown = breakdown.join(", ");
    format!(
        "Plugins: {plugin_count} ({breakdown})\nAgents: {total_agents} | Skills: {total_skills}\n"
    )
}

fn intent_summary_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    if !root.has_intents {
        return String::new();
    }

    let Ok(intents) = scan_intents(root, fs) else {
        return String::new();
    };
    if intents.is_empty() {
        return String::new();
    }

    let mut by_category: BTreeMap<String, usize> = BTreeMap::new();
    for intent in &intents {
        *by_category.entry(intent.record.category.clone()).or_default() += 1;
    }
    let breakdown = by_category
        .iter()
        .map(|(category, count)| format!("{count} {category}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Intents: {} ({breakdown})\n", intents.len())
}

fn validation_summary_str(root: &RepoRoot, fs: &dyn Filesystem) -> String {
    let Ok(issues) = validate_all(root, fs) else {
        return String::new();
    };

    if issues.is_empty() {
        return "Validation: all clean\n".to_string();
    }

    let errors = issues.iter().filter(|i| i.level == IssueLevel::Error).count();
    let warnings = issues.iter().filter(|i| i.level == IssueLevel::Warning).count();

    let mut parts = Vec::new();
    if errors > 0 {
        let label = if errors == 1 { "error" } else { "errors" };
        parts.push(format!("{errors} {label}"));
    }
    if warnings > 0 {
        let label = if warnings == 1 { "warning" } else { "warnings" };
        parts.push(format!("{warnings} {label}"));
    }

    let summary = parts.join(", ");
    format!("Validation: {summary}\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cmx::gateway::fakes::FakeFilesystem;

    use crate::repo::{RepoKind, RepoRoot};
    use crate::test_support::{
        fake_marketplace_json, fake_marketplace_root_simple, fake_plugin_json,
    };

    use super::{
        intent_summary_str, plugin_summary_str, repo_identity_str, status_report,
        validation_summary_str,
    };

    fn unknown_root(path: &str) -> RepoRoot {
        RepoRoot {
            path: PathBuf::from(path),
            kind: RepoKind::Unknown,
            has_intents: false,
            has_plugins_dir: false,
        }
    }

    #[test]
    fn repo_identity_str_marketplace_with_name() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            r#"{"name":"My Marketplace","plugins":[]}"#,
        );
        let root = fake_marketplace_root_simple("/repo");
        let out = repo_identity_str(&root, &fs);
        assert!(out.contains("My Marketplace"));
        assert!(out.contains("marketplace"));
    }

    #[test]
    fn repo_identity_str_plugin_without_name() {
        let fs = FakeFilesystem::new();
        let root = RepoRoot {
            path: PathBuf::from("/plugin"),
            kind: RepoKind::Plugin,
            has_intents: false,
            has_plugins_dir: false,
        };
        let out = repo_identity_str(&root, &fs);
        assert!(out.contains("(plugin)"));
    }

    #[test]
    fn repo_identity_str_unknown_kind() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/somewhere");
        let out = repo_identity_str(&root, &fs);
        assert!(out.contains("(unknown)"));
    }

    #[test]
    fn plugin_summary_str_no_plugins_returns_empty() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", r#"{"name":"empty","plugins":[]}"#);
        let root = fake_marketplace_root_simple("/repo");
        assert_eq!(plugin_summary_str(&root, &fs), "");
    }

    #[test]
    fn plugin_summary_str_with_plugins_shows_counts() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("my-plugin", "A plugin", "./plugins/my-plugin")]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", json.as_str());
        fs.add_dir("/repo/plugins/my-plugin");
        fs.add_file(
            "/repo/plugins/my-plugin/.claude-plugin/plugin.json",
            fake_plugin_json("my-plugin"),
        );
        let root = fake_marketplace_root_simple("/repo");
        let out = plugin_summary_str(&root, &fs);
        assert!(out.contains("Plugins:"));
        assert!(out.contains('1'));
    }

    #[test]
    fn intent_summary_str_no_intents_flag_returns_empty() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/repo");
        assert_eq!(intent_summary_str(&root, &fs), "");
    }

    #[test]
    fn intent_summary_str_with_intents_shows_count() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/intents/craftsperson/verify.toml",
            crate::test_support::fake_intent_record("guidelines.intent.verify"),
        );
        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::IntentsOnly,
            has_intents: true,
            has_plugins_dir: false,
        };
        let out = intent_summary_str(&root, &fs);
        assert!(out.contains("Intents:"));
        assert!(out.contains('1'));
    }

    #[test]
    fn validation_summary_str_all_clean() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", r#"{"name":"clean","plugins":[]}"#);
        let root = fake_marketplace_root_simple("/repo");
        assert_eq!(validation_summary_str(&root, &fs), "Validation: all clean\n");
    }

    #[test]
    fn validation_summary_str_with_errors() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/nowhere");
        let out = validation_summary_str(&root, &fs);
        assert!(out.contains("Validation:"));
        assert!(out.contains("error"));
    }

    #[test]
    fn validation_summary_str_warnings_only() {
        let fs = FakeFilesystem::new();
        let json = fake_marketplace_json(&[("my-plugin", "A plugin", "./plugins/my-plugin")]);
        fs.add_file("/repo/.claude-plugin/marketplace.json", json.as_str());
        fs.add_dir("/repo/plugins/my-plugin");
        fs.add_file(
            "/repo/plugins/my-plugin/.claude-plugin/plugin.json",
            fake_plugin_json("mismatched-name"),
        );
        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Marketplace,
            has_intents: false,
            has_plugins_dir: true,
        };
        let out = validation_summary_str(&root, &fs);
        assert!(out.contains("Validation:"));
        assert!(out.contains("warning"));
    }

    #[test]
    fn status_report_clean_marketplace() {
        let fs = FakeFilesystem::new();
        fs.add_file(
            "/repo/.claude-plugin/marketplace.json",
            r#"{"name":"My Marketplace","plugins":[]}"#,
        );
        let root = fake_marketplace_root_simple("/repo");
        let out = status_report(&root, &fs);
        assert!(out.contains("My Marketplace"));
        assert!(out.contains("Validation: all clean"));
    }

    #[test]
    fn status_report_unknown_repo_includes_identity() {
        let fs = FakeFilesystem::new();
        let root = unknown_root("/somewhere");
        let out = status_report(&root, &fs);
        assert!(out.contains("(unknown)"));
    }
}
