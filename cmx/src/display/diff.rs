#[cfg(feature = "llm")]
use crate::diff::DiffOutput;
#[cfg(feature = "llm")]
use std::fmt;

#[cfg(feature = "llm")]
impl fmt::Display for DiffOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_up_to_date {
            return writeln!(f, "{} is up to date with source.", self.artifact_name);
        }

        let installed_ver = self.installed_version.as_deref().unwrap_or("unversioned");
        let source_ver = self.source_version.as_deref().unwrap_or("unversioned");

        writeln!(f, "Comparing {} ({})", self.artifact_name, self.kind)?;
        writeln!(f, "  Installed: {installed_ver}")?;
        writeln!(f, "  Source ({}): {source_ver}", self.source_name)?;
        writeln!(f)?;

        if let Some(analysis) = &self.analysis {
            writeln!(f, "Analyzing differences...")?;
            writeln!(f)?;
            writeln!(f, "{analysis}")?;
        } else if let Some(diff) = &self.diff_text {
            writeln!(f, "Differences:")?;
            writeln!(f, "{diff}")?;
        }

        Ok(())
    }
}

#[cfg(all(test, feature = "llm"))]
mod tests {
    use super::*;
    use crate::types::ArtifactKind;

    // --- Step 11: DiffOutput (feature-gated) ---

    #[test]
    fn diff_output_is_up_to_date_message() {
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: true,
            installed_version: None,
            source_version: None,
            source_name: "src".to_string(),
            diff_text: None,
            analysis: None,
        };
        assert!(r.to_string().contains("is up to date with source."));
    }

    #[test]
    fn diff_output_with_analysis_shows_analyzing() {
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: false,
            installed_version: Some("1.0.0".to_string()),
            source_version: Some("2.0.0".to_string()),
            source_name: "src".to_string(),
            diff_text: Some("--- a\n+++ b\n".to_string()),
            analysis: Some("Notable changes found.".to_string()),
        };
        let out = r.to_string();
        assert!(out.contains("Analyzing differences..."));
        assert!(out.contains("Notable changes found."));
    }

    #[test]
    fn diff_output_diff_text_only_shows_differences() {
        let r = DiffOutput {
            artifact_name: "my-agent".to_string(),
            kind: ArtifactKind::Agent,
            is_up_to_date: false,
            installed_version: None,
            source_version: None,
            source_name: "src".to_string(),
            diff_text: Some("--- a\n+++ b\n".to_string()),
            analysis: None,
        };
        assert!(r.to_string().contains("Differences:"));
    }
}
