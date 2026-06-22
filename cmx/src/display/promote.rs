use std::fmt;

use crate::promote::PromoteResult;

fn platforms_label(platforms: &[crate::platform::Platform]) -> String {
    platforms.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

impl fmt::Display for PromoteResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.already_current {
            return writeln!(f, "'{}' already matches the home — nothing to promote.", self.name);
        }

        let version = self.version.as_deref().unwrap_or("unversioned");
        writeln!(
            f,
            "Promoted '{}' ({version}) into the home: {}",
            self.name,
            self.home_path.display()
        )?;
        writeln!(f, "  re-tracked for: {}", platforms_label(&self.retracked))?;

        if !self.still_divergent.is_empty() {
            writeln!(
                f,
                "  note: {} still differ(s) from the promoted copy and now read(s) as drifted — \
                 reconcile with `cmx {} sync {}` or promote from there.",
                platforms_label(&self.still_divergent),
                self.kind,
                self.name,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::platform::Platform;
    use crate::promote::PromoteResult;
    use crate::types::ArtifactKind;
    use std::path::PathBuf;

    #[test]
    fn promote_already_current_message() {
        let r = PromoteResult {
            name: "personal-finance".to_string(),
            kind: ArtifactKind::Skill,
            home_path: PathBuf::from("/home/skills/personal-finance"),
            already_current: true,
            version: None,
            retracked: vec![],
            still_divergent: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("already matches the home"), "got: {out}");
    }

    #[test]
    fn promote_reports_home_path_and_retracked_platforms() {
        let r = PromoteResult {
            name: "personal-finance".to_string(),
            kind: ArtifactKind::Skill,
            home_path: PathBuf::from("/home/skills/personal-finance"),
            already_current: false,
            version: Some("1.2.0".to_string()),
            retracked: vec![Platform::Claude, Platform::Codex],
            still_divergent: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("Promoted 'personal-finance' (1.2.0)"), "got: {out}");
        assert!(out.contains("/home/skills/personal-finance"), "shows home path: {out}");
        assert!(out.contains("claude, codex"), "lists re-tracked platforms: {out}");
        assert!(!out.contains("still differ"), "no divergence note when all agree: {out}");
    }

    #[test]
    fn promote_warns_about_still_divergent_platforms() {
        let r = PromoteResult {
            name: "personal-finance".to_string(),
            kind: ArtifactKind::Skill,
            home_path: PathBuf::from("/home/skills/personal-finance"),
            already_current: false,
            version: None,
            retracked: vec![Platform::Claude, Platform::Codex],
            still_divergent: vec![Platform::Codex],
        };
        let out = r.to_string();
        assert!(out.contains("still differ"), "warns about divergence: {out}");
        assert!(out.contains("cmx skill sync personal-finance"), "points at sync: {out}");
    }
}
