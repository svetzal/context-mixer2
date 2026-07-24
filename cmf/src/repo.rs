//! Repo root detection (marketplace, plugin, facets-only, unknown).

use std::path::{Path, PathBuf};

use anyhow::Result;
use cmx::gateway::Filesystem;

/// The kind of repository root detected by [`detect_repo`], based on which
/// marker files/directories are present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoKind {
    /// Has `.claude-plugin/marketplace.json`
    Marketplace,
    /// Has `.claude-plugin/plugin.json` but no `marketplace.json`
    Plugin,
    /// Has `facets/` but no `.claude-plugin/`
    FacetsOnly,
    /// No recognized markers
    Unknown,
}

/// Result of detecting a repository's kind and structure at a given path.
#[derive(Debug, Clone)]
pub struct RepoRoot {
    /// Absolute path to the repository root that was inspected.
    pub path: PathBuf,
    /// The detected repository kind.
    pub kind: RepoKind,
    /// Whether a `facets/` directory exists at the root.
    pub has_facets: bool,
    /// Whether a `plugins/` directory exists at the root.
    pub has_plugins_dir: bool,
}

/// Detect the repository kind by looking for marker files/directories at `start`.
///
/// Does not walk upward — only inspects the given directory.
pub fn detect_repo(start: &Path, fs: &dyn Filesystem) -> Result<RepoRoot> {
    let marketplace_json = start.join(".claude-plugin").join("marketplace.json");
    let plugin_json = start.join(".claude-plugin").join("plugin.json");
    let facets_dir = start.join("facets");
    let plugins_dir = start.join("plugins");

    let has_marketplace = fs.exists(&marketplace_json);
    let has_plugin = fs.exists(&plugin_json);
    let has_facets = fs.is_dir(&facets_dir);
    let has_plugins_dir = fs.is_dir(&plugins_dir);

    let kind = if has_marketplace {
        RepoKind::Marketplace
    } else if has_plugin {
        RepoKind::Plugin
    } else if has_facets {
        RepoKind::FacetsOnly
    } else {
        RepoKind::Unknown
    };

    Ok(RepoRoot {
        path: start.to_path_buf(),
        kind,
        has_facets,
        has_plugins_dir,
    })
}

/// Resolve a marketplace source path (which may start with `./`) relative to
/// the repository root.
pub fn resolve_source_path(root: &Path, source: &str) -> PathBuf {
    let cleaned = source.strip_prefix("./").unwrap_or(source);
    root.join(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx::gateway::fakes::FakeFilesystem;

    #[test]
    fn detect_marketplace_repo() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", "{}");
        let root = detect_repo(Path::new("/repo"), &fs).unwrap();
        assert_eq!(root.kind, RepoKind::Marketplace);
        assert_eq!(root.path, PathBuf::from("/repo"));
    }

    #[test]
    fn detect_plugin_repo() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/plugin.json", "{}");
        let root = detect_repo(Path::new("/repo"), &fs).unwrap();
        assert_eq!(root.kind, RepoKind::Plugin);
    }

    #[test]
    fn detect_facets_only() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo/facets");
        let root = detect_repo(Path::new("/repo"), &fs).unwrap();
        assert_eq!(root.kind, RepoKind::FacetsOnly);
        assert!(root.has_facets);
    }

    #[test]
    fn detect_unknown() {
        let fs = FakeFilesystem::new();
        fs.add_dir("/repo");
        let root = detect_repo(Path::new("/repo"), &fs).unwrap();
        assert_eq!(root.kind, RepoKind::Unknown);
        assert!(!root.has_facets);
        assert!(!root.has_plugins_dir);
    }

    #[test]
    fn marketplace_with_facets() {
        let fs = FakeFilesystem::new();
        fs.add_file("/repo/.claude-plugin/marketplace.json", "{}");
        fs.add_dir("/repo/facets");
        let root = detect_repo(Path::new("/repo"), &fs).unwrap();
        assert_eq!(root.kind, RepoKind::Marketplace);
        assert!(root.has_facets);
    }
}
