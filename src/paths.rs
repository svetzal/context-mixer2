use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::types::ArtifactKind;

/// Centralizes all path resolution for cmx configuration and install directories.
///
/// Production code constructs this via [`ConfigPaths::from_env`]; tests use
/// [`ConfigPaths::for_test`] to inject arbitrary root directories and avoid
/// touching the real home directory.
pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub home_dir: PathBuf,
}

impl ConfigPaths {
    /// Production constructor — derives paths from the real home and config directories.
    pub fn from_env() -> Result<Self> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        let config_dir = home.join(".config").join("context-mixer");
        Ok(Self {
            config_dir,
            home_dir: home,
        })
    }

    /// Test constructor — uses arbitrary root directories so no real home
    /// directory is touched.
    pub fn for_test(home: PathBuf, config: PathBuf) -> Self {
        Self {
            config_dir: config,
            home_dir: home,
        }
    }

    /// Path to `sources.json`.
    pub fn sources_path(&self) -> PathBuf {
        self.config_dir.join("sources.json")
    }

    /// Directory where git-backed sources are cloned.
    pub fn git_clones_dir(&self) -> PathBuf {
        self.config_dir.join("sources")
    }

    /// Path to `config.json` (LLM gateway settings).
    pub fn config_path(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    /// Path to the lock file for the given scope.
    pub fn lock_path(&self, local: bool) -> PathBuf {
        if local {
            PathBuf::from(".context-mixer").join("cmx-lock.json")
        } else {
            self.config_dir.join("cmx-lock.json")
        }
    }

    /// Directory where artifacts of the given kind and scope are installed.
    pub fn install_dir(&self, kind: ArtifactKind, local: bool) -> PathBuf {
        let subdir = match kind {
            ArtifactKind::Agent => "agents",
            ArtifactKind::Skill => "skills",
        };
        if local {
            PathBuf::from(".claude").join(subdir)
        } else {
            self.home_dir.join(".claude").join(subdir)
        }
    }
}
