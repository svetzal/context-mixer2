use crate::plugin::PluginInfo;

pub fn print_plugin_list(plugins: &[PluginInfo]) {
    println!("Plugins ({}):", plugins.len());

    if plugins.is_empty() {
        return;
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

        println!(
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
}
