use std::fmt;

use crate::plugin::PluginList;

impl fmt::Display for PluginList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plugins = &self.0;
        writeln!(f, "Plugins ({}):", plugins.len())?;

        if plugins.is_empty() {
            return Ok(());
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
            writeln!(
                f,
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
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::plugin::{PluginInfo, PluginList};

    #[test]
    fn plugin_list_display_empty() {
        assert!(PluginList(vec![]).to_string().starts_with("Plugins (0):"));
    }

    #[test]
    fn plugin_list_display_single_plugin() {
        let plugin = PluginInfo {
            name: "rust-craft".to_string(),
            version: Some("1.0.0".to_string()),
            description: None,
            category: Some("dev".to_string()),
            path: PathBuf::from("/plugins/rust-craft"),
            agents: vec![],
            skills: vec![],
        };
        let out = PluginList(vec![plugin]).to_string();
        assert!(out.contains("Plugins (1):"));
        assert!(out.contains("rust-craft"));
        assert!(out.contains("1.0.0"));
    }

    #[test]
    fn plugin_list_display_optional_fields_absent() {
        let plugin = PluginInfo {
            name: "bare-plugin".to_string(),
            version: None,
            description: None,
            category: None,
            path: PathBuf::from("/plugins/bare-plugin"),
            agents: vec![],
            skills: vec![],
        };
        let out = PluginList(vec![plugin]).to_string();
        assert!(out.contains("bare-plugin"));
        assert!(out.contains('-'));
    }
}
