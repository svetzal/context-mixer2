use std::fmt;

use crate::install::{BatchInstallResult, InstallManyResult, InstallResult};
use crate::types::format_version_prefix;

use super::util;

fn write_discarded_paths(f: &mut fmt::Formatter<'_>, result: &InstallResult) -> fmt::Result {
    for path in &result.discarded_paths {
        writeln!(f, "Discarding local modification: {}", path.display())?;
    }
    Ok(())
}

impl fmt::Display for InstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_discarded_paths(f, self)?;
        let version_info = format_version_prefix(self.version.as_deref());
        writeln!(
            f,
            "Installed {}{version_info} ({}) for {} from '{}' -> {}",
            self.artifact_name,
            self.kind,
            self.platform,
            self.source_name,
            self.dest_dir.display()
        )
    }
}

impl fmt::Display for BatchInstallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.items.is_empty() {
            if self.is_update {
                writeln!(f, "All tracked {}s are up to date.", self.kind)
            } else {
                writeln!(f, "All available {}s are already installed and up to date.", self.kind)
            }
        } else {
            util::write_each(f, &self.items)
        }
    }
}

impl fmt::Display for InstallManyResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        util::write_each(f, &self.installed)?;
        for (name, reason) in &self.failed {
            writeln!(f, "Failed: {name} — {reason}")?;
        }
        if self.installed.is_empty() && self.failed.is_empty() {
            writeln!(f, "No {}s given to install.", self.kind)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ArtifactKind;
    use std::path::PathBuf;

    // --- Step 5: InstallResult and BatchInstallResult ---

    #[test]
    fn install_result_with_version_includes_version_prefix() {
        let r = InstallResult {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            source_name: "guidelines".to_string(),
            dest_dir: PathBuf::from("/home/user/.claude/agents"),
            version: Some("1.2.3".to_string()),
            platform: crate::platform::Platform::Claude,
            discarded_paths: Vec::new(),
        };
        let out = r.to_string();
        assert!(out.contains("my-agent"));
        assert!(out.contains("v1.2.3"));
        assert!(out.contains("guidelines"));
    }

    #[test]
    fn batch_install_result_empty_update_up_to_date() {
        let r = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
            is_update: true,
        };
        assert!(r.to_string().contains("up to date"));
    }

    #[test]
    fn batch_install_result_empty_install_already_installed() {
        let r = BatchInstallResult {
            items: vec![],
            kind: ArtifactKind::Agent,
            is_update: false,
        };
        assert!(r.to_string().contains("already installed"));
    }

    #[test]
    fn batch_install_result_with_items_delegates_to_install_result() {
        let r = BatchInstallResult {
            items: vec![InstallResult {
                artifact_name: "my-skill".to_string(),
                kind: ArtifactKind::Skill,
                source_name: "src".to_string(),
                dest_dir: PathBuf::from("/home/user/.claude/skills"),
                version: None,
                platform: crate::platform::Platform::Claude,
                discarded_paths: Vec::new(),
            }],
            kind: ArtifactKind::Skill,
            is_update: false,
        };
        let out = r.to_string();
        assert!(out.contains("my-skill"));
        assert!(out.contains("src"));
    }
}
