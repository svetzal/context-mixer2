use std::path::PathBuf;

use anyhow::Result;
use cmx::gateway::Filesystem;

use crate::repo::RepoRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Claude,
    Codex,
    Cursor,
    Gemini,
}

impl Platform {
    /// The directory name for this platform's plugin manifest.
    pub fn manifest_dir(&self) -> &str {
        match self {
            Self::Claude => ".claude-plugin",
            Self::Codex => ".codex-plugin",
            Self::Cursor => ".cursor-plugin",
            Self::Gemini => ".gemini-plugin",
        }
    }

    /// All non-Claude platforms that we generate for.
    pub fn targets() -> &'static [Platform] {
        &[Self::Codex, Self::Cursor, Self::Gemini]
    }
}

/// Generate multi-platform manifests from the canonical `.claude-plugin/` source.
///
/// Works at both marketplace level (root `marketplace.json`) and per-plugin level
/// (`plugin.json`). Returns the list of files that were written.
pub fn generate_manifests(root: &RepoRoot, fs: &dyn Filesystem) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();

    // Collect all source files to replicate: (source_path, containing_dir)
    // where containing_dir is the parent of `.claude-plugin/`
    let mut sources: Vec<(PathBuf, PathBuf)> = Vec::new();

    // Root-level marketplace.json
    let root_marketplace = root.path.join(".claude-plugin").join("marketplace.json");
    if fs.exists(&root_marketplace) {
        sources.push((root_marketplace, root.path.clone()));
    }

    // Root-level plugin.json (for single-plugin repos)
    let root_plugin = root.path.join(".claude-plugin").join("plugin.json");
    if fs.exists(&root_plugin) {
        sources.push((root_plugin, root.path.clone()));
    }

    // Per-plugin plugin.json files under plugins/
    let plugins_dir = root.path.join("plugins");
    if fs.is_dir(&plugins_dir) {
        if let Ok(entries) = fs.read_dir(&plugins_dir) {
            for entry in entries {
                if !entry.is_dir {
                    continue;
                }
                let plugin_json = entry.path.join(".claude-plugin").join("plugin.json");
                if fs.exists(&plugin_json) {
                    sources.push((plugin_json, entry.path.clone()));
                }
            }
        }
    }

    // For each source file, copy to each target platform directory
    for (source_path, containing_dir) in &sources {
        let content = fs.read_to_string(source_path)?;
        let file_name = source_path
            .file_name()
            .expect("source path should have a file name")
            .to_string_lossy();

        for platform in Platform::targets() {
            let target_dir = containing_dir.join(platform.manifest_dir());
            fs.create_dir_all(&target_dir)?;

            let target_path = target_dir.join(file_name.as_ref());
            fs.write(&target_path, &content)?;
            written.push(target_path);
        }
    }

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::RepoKind;
    use crate::test_support::{fake_marketplace_json, fake_plugin_json};
    use cmx::gateway::fakes::FakeFilesystem;

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
    fn platform_manifest_dir_values() {
        assert_eq!(Platform::Claude.manifest_dir(), ".claude-plugin");
        assert_eq!(Platform::Codex.manifest_dir(), ".codex-plugin");
        assert_eq!(Platform::Cursor.manifest_dir(), ".cursor-plugin");
        assert_eq!(Platform::Gemini.manifest_dir(), ".gemini-plugin");
    }

    #[test]
    fn generate_creates_all_platform_dirs() {
        let fs = FakeFilesystem::new();
        let marketplace_json = fake_marketplace_json(&[
            ("alpha", "Alpha plugin", "./plugins/alpha"),
            ("beta", "Beta plugin", "./plugins/beta"),
        ]);
        let root = marketplace_root(&fs, &marketplace_json);

        // Set up per-plugin manifests
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));
        fs.add_file("/repo/plugins/beta/.claude-plugin/plugin.json", fake_plugin_json("beta"));

        generate_manifests(&root, &fs).unwrap();

        // Root-level platform dirs should exist
        for platform in Platform::targets() {
            let dir = PathBuf::from("/repo").join(platform.manifest_dir());
            assert!(fs.is_dir(&dir), "expected root-level dir {} to exist", dir.display());
        }

        // Per-plugin platform dirs should exist
        for plugin in &["alpha", "beta"] {
            for platform in Platform::targets() {
                let dir =
                    PathBuf::from(format!("/repo/plugins/{plugin}")).join(platform.manifest_dir());
                assert!(fs.is_dir(&dir), "expected plugin dir {} to exist", dir.display());
            }
        }
    }

    #[test]
    fn generate_copies_marketplace_json() {
        let fs = FakeFilesystem::new();
        let marketplace_json =
            fake_marketplace_json(&[("alpha", "Alpha plugin", "./plugins/alpha")]);
        let root = marketplace_root(&fs, &marketplace_json);

        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));

        generate_manifests(&root, &fs).unwrap();

        let source_content = fs
            .read_to_string(&PathBuf::from("/repo/.claude-plugin/marketplace.json"))
            .unwrap();

        for platform in Platform::targets() {
            let target_path =
                PathBuf::from("/repo").join(platform.manifest_dir()).join("marketplace.json");
            let target_content = fs.read_to_string(&target_path).unwrap();
            assert_eq!(
                source_content,
                target_content,
                "marketplace.json content should match for {}",
                platform.manifest_dir()
            );
        }
    }

    #[test]
    fn generate_copies_plugin_json() {
        let fs = FakeFilesystem::new();
        let marketplace_json =
            fake_marketplace_json(&[("alpha", "Alpha plugin", "./plugins/alpha")]);
        let root = marketplace_root(&fs, &marketplace_json);

        let plugin_content = fake_plugin_json("alpha");
        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", plugin_content.as_str());

        generate_manifests(&root, &fs).unwrap();

        for platform in Platform::targets() {
            let target_path = PathBuf::from("/repo/plugins/alpha")
                .join(platform.manifest_dir())
                .join("plugin.json");
            let target_content = fs.read_to_string(&target_path).unwrap();
            assert_eq!(
                plugin_content,
                target_content,
                "plugin.json content should match for {}",
                platform.manifest_dir()
            );
        }
    }

    #[test]
    fn generate_returns_all_written_paths() {
        let fs = FakeFilesystem::new();
        let marketplace_json = fake_marketplace_json(&[
            ("alpha", "Alpha plugin", "./plugins/alpha"),
            ("beta", "Beta plugin", "./plugins/beta"),
        ]);
        let root = marketplace_root(&fs, &marketplace_json);

        fs.add_file("/repo/plugins/alpha/.claude-plugin/plugin.json", fake_plugin_json("alpha"));
        fs.add_file("/repo/plugins/beta/.claude-plugin/plugin.json", fake_plugin_json("beta"));

        let written = generate_manifests(&root, &fs).unwrap();

        // 3 platforms x (1 marketplace.json + 2 per-plugin plugin.json) = 9
        assert_eq!(
            written.len(),
            9,
            "expected 3 platforms x (1 marketplace + 2 plugins), got: {written:?}"
        );

        // Verify each platform has the right count
        for platform in Platform::targets() {
            let count = written
                .iter()
                .filter(|p| p.components().any(|c| c.as_os_str() == platform.manifest_dir()))
                .count();
            assert_eq!(count, 3, "expected 3 files for {}, got {count}", platform.manifest_dir());
        }
    }

    #[test]
    fn generate_skips_if_no_claude_source() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");
        let root = RepoRoot {
            path: PathBuf::from("/repo"),
            kind: RepoKind::Unknown,
            has_facets: false,
            has_plugins_dir: false,
        };

        let written = generate_manifests(&root, &fs).unwrap();
        assert!(written.is_empty(), "expected no files written when no .claude-plugin/ exists");
    }
}
