use std::fmt;

use cmx::platform::Platform;
use cmx::table::empty_state;

use crate::manifest::ManifestSummary;

impl fmt::Display for ManifestSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let files = &self.0;
        if files.is_empty() {
            return write!(
                f,
                "{}",
                empty_state("No .claude-plugin/ sources found — nothing to generate.")
            );
        }

        writeln!(f, "Generated manifests for {} platforms:", Platform::targets().len())?;

        for (_platform, dir_name) in Platform::manifest_targets() {
            let platform_files: Vec<_> = files
                .iter()
                .filter(|p| p.components().any(|c| c.as_os_str() == dir_name))
                .collect();

            let marketplace_count =
                platform_files.iter().filter(|p| p.ends_with("marketplace.json")).count();
            let plugin_count = platform_files.iter().filter(|p| p.ends_with("plugin.json")).count();

            let mut parts = Vec::new();
            if marketplace_count > 0 {
                parts.push("marketplace.json".to_string());
            }
            if plugin_count > 0 {
                parts.push(format!(
                    "{plugin_count} plugin manifest{}",
                    if plugin_count == 1 { "" } else { "s" }
                ));
            }

            writeln!(f, "  {dir_name}/ — {}", parts.join(" + "))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cmx::platform::Platform;

    use crate::manifest::ManifestSummary;

    #[test]
    fn manifest_summary_display_empty() {
        let out = ManifestSummary(vec![]).to_string();
        assert!(out.contains("nothing to generate"));
    }

    #[test]
    fn manifest_summary_display_with_files() {
        let dir = Platform::targets()[0]
            .manifest_dir()
            .expect("targets() platforms have a manifest_dir");
        let files = vec![
            PathBuf::from(format!("/{dir}/marketplace.json")),
            PathBuf::from(format!("/{dir}/plugin.json")),
        ];
        let out = ManifestSummary(files).to_string();
        assert!(out.contains("Generated manifests"));
        assert!(out.contains(dir));
    }
}
