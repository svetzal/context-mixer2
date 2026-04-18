use std::fmt::Write as FmtWrite;

use crate::plugin::PluginInfo;

pub fn format_plugin_list(plugins: &[PluginInfo]) -> String {
    let mut out = format!("Plugins ({}):\n", plugins.len());

    if plugins.is_empty() {
        return out;
    }

    let max_name = plugins.iter().map(|p| p.name.len()).max().unwrap_or(0);
    let max_version = plugins
        .iter()
        .map(|p| p.version.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(0);
    let max_category = plugins
        .iter()
        .map(|p| p.category.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(0);

    for plugin in plugins {
        let version = plugin.version.as_deref().unwrap_or("-");
        let category = plugin.category.as_deref().unwrap_or("-");
        let agents = plugin.agents.len();
        let skills = plugin.skills.len();

        let _ = writeln!(
            out,
            "  {:<name_w$}  {:>ver_w$}  {:<cat_w$}  {} {}  {} {}",
            plugin.name,
            version,
            category,
            agents,
            if agents == 1 { "agent " } else { "agents" },
            skills,
            if skills == 1 { "skill " } else { "skills" },
            name_w = max_name,
            ver_w = max_version,
            cat_w = max_category,
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::plugin::PluginInfo;
    use cmx::types::{Artifact, ArtifactKind};

    fn fake_artifact(kind: ArtifactKind) -> Artifact {
        Artifact {
            kind,
            name: "test".to_string(),
            description: String::new(),
            path: PathBuf::from("/tmp/test"),
            version: None,
            deprecation: None,
        }
    }

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
            agents: (0..agents).map(|_| fake_artifact(ArtifactKind::Agent)).collect(),
            skills: (0..skills).map(|_| fake_artifact(ArtifactKind::Skill)).collect(),
        }
    }

    #[test]
    fn format_plugin_list_empty() {
        let out = format_plugin_list(&[]);
        assert_eq!(out, "Plugins (0):\n");
    }

    #[test]
    fn format_plugin_list_single_plugin() {
        let plugins = vec![make_plugin("my-plugin", Some("1.0.0"), Some("tools"), 1, 2)];
        let out = format_plugin_list(&plugins);
        assert!(out.starts_with("Plugins (1):"));
        assert!(out.contains("my-plugin"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("tools"));
        assert!(out.contains("1 agent "));
        assert!(out.contains("2 skills"));
    }

    #[test]
    fn format_plugin_list_plural_agents() {
        let plugins = vec![make_plugin("my-plugin", None, None, 2, 1)];
        let out = format_plugin_list(&plugins);
        assert!(out.contains("2 agents"));
        assert!(out.contains("1 skill "));
    }

    #[test]
    fn format_plugin_list_missing_optional_fields() {
        let plugins = vec![make_plugin("my-plugin", None, None, 0, 0)];
        let out = format_plugin_list(&plugins);
        assert!(out.contains("my-plugin"));
        assert!(out.contains('-'));
    }
}
