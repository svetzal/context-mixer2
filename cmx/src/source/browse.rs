use std::collections::HashMap;
use std::path::PathBuf;

use crate::gateway::DirEntry;
use crate::types::{Artifact, ArtifactKind};

pub struct BrowseArtifact {
    pub name: String,
    pub version: Option<String>,
    pub deprecation_display: String,
}

pub struct BrowseSkill {
    pub name: String,
    pub version: Option<String>,
    pub deprecation_display: String,
    pub files: Vec<String>,
}

pub struct SourceBrowseResult {
    pub source_name: String,
    pub agents: Vec<BrowseArtifact>,
    pub skills: Vec<BrowseSkill>,
}

/// Build a `SourceBrowseResult` from pre-loaded data with no filesystem access.
pub(crate) fn build_browse_result(
    source_name: &str,
    artifacts: &[Artifact],
    skill_dirs: &HashMap<PathBuf, Vec<String>>,
) -> SourceBrowseResult {
    let agents = artifacts_of_kind(artifacts, ArtifactKind::Agent)
        .map(|a| BrowseArtifact {
            name: a.name.clone(),
            version: a.version.clone(),
            deprecation_display: format_deprecation(a),
        })
        .collect();

    let skills = artifacts_of_kind(artifacts, ArtifactKind::Skill)
        .map(|s| {
            let files = skill_dirs.get(&s.path).cloned().unwrap_or_default();
            build_browse_skill(s, files)
        })
        .collect();

    SourceBrowseResult {
        source_name: source_name.to_string(),
        agents,
        skills,
    }
}

fn artifacts_of_kind(
    artifacts: &[Artifact],
    kind: ArtifactKind,
) -> impl Iterator<Item = &Artifact> {
    artifacts.iter().filter(move |a| a.kind == kind)
}

pub(crate) fn dir_entry_names(entries: &[DirEntry]) -> Vec<String> {
    let mut names: Vec<String> = entries
        .iter()
        .filter(|e| !e.file_name.starts_with('.'))
        .map(|e| {
            if e.is_dir {
                format!("{}/", e.file_name)
            } else {
                e.file_name.clone()
            }
        })
        .collect();
    names.sort();
    names
}

fn build_browse_skill(artifact: &Artifact, files: Vec<String>) -> BrowseSkill {
    BrowseSkill {
        name: artifact.name.clone(),
        version: artifact.version.clone(),
        deprecation_display: format_deprecation(artifact),
        files,
    }
}

pub(crate) fn count_artifacts(artifacts: &[Artifact]) -> (usize, usize) {
    let agents = artifacts_of_kind(artifacts, ArtifactKind::Agent).count();
    let skills = artifacts_of_kind(artifacts, ArtifactKind::Skill).count();
    (agents, skills)
}

pub(super) fn format_deprecation(artifact: &Artifact) -> String {
    let Some(dep) = &artifact.deprecation else {
        return String::new();
    };

    let mut parts = vec!["  ⛔ DEPRECATED".to_string()];

    if let Some(reason) = &dep.reason {
        parts.push(format!(": {reason}"));
    }

    if let Some(replacement) = &dep.replacement {
        parts.push(format!(" (use {replacement} instead)"));
    }

    parts.join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Deprecation;

    fn make_agent(name: &str) -> Artifact {
        Artifact {
            kind: ArtifactKind::Agent,
            name: name.to_string(),
            description: String::new(),
            path: PathBuf::from(format!("{name}.md")),
            version: None,
            deprecation: None,
        }
    }

    fn make_skill(name: &str) -> Artifact {
        Artifact {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            description: String::new(),
            path: PathBuf::from(name),
            version: None,
            deprecation: None,
        }
    }

    // --- count_artifacts ---

    #[test]
    fn count_artifacts_empty() {
        assert_eq!(count_artifacts(&[]), (0, 0));
    }

    #[test]
    fn count_artifacts_only_agents() {
        let arts = vec![make_agent("alpha"), make_agent("beta")];
        assert_eq!(count_artifacts(&arts), (2, 0));
    }

    #[test]
    fn count_artifacts_mixed() {
        let arts = vec![make_agent("alpha"), make_skill("zap"), make_skill("zip")];
        assert_eq!(count_artifacts(&arts), (1, 2));
    }

    // --- format_deprecation ---

    #[test]
    fn format_deprecation_not_deprecated() {
        let artifact = make_agent("alpha");
        assert_eq!(format_deprecation(&artifact), "");
    }

    #[test]
    fn format_deprecation_deprecated_no_extras() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: None,
                replacement: None,
            }),
        };
        assert_eq!(format_deprecation(&artifact), "  ⛔ DEPRECATED");
    }

    #[test]
    fn format_deprecation_deprecated_with_reason() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: Some("Too old".to_string()),
                replacement: None,
            }),
        };
        assert_eq!(format_deprecation(&artifact), "  ⛔ DEPRECATED: Too old");
    }

    #[test]
    fn format_deprecation_deprecated_with_reason_and_replacement() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: Some("Too old".to_string()),
                replacement: Some("new-agent".to_string()),
            }),
        };
        assert_eq!(
            format_deprecation(&artifact),
            "  ⛔ DEPRECATED: Too old (use new-agent instead)"
        );
    }

    #[test]
    fn format_deprecation_deprecated_with_replacement_only() {
        let artifact = Artifact {
            kind: ArtifactKind::Agent,
            name: "alpha".to_string(),
            description: String::new(),
            path: PathBuf::from("alpha.md"),
            version: None,
            deprecation: Some(Deprecation {
                reason: None,
                replacement: Some("new-agent".to_string()),
            }),
        };
        assert_eq!(format_deprecation(&artifact), "  ⛔ DEPRECATED (use new-agent instead)");
    }

    // --- dir_entry_names ---

    fn make_dir_entry(file_name: &str, is_dir: bool) -> crate::gateway::DirEntry {
        crate::gateway::DirEntry {
            path: PathBuf::from(file_name),
            file_name: file_name.to_string(),
            is_dir,
        }
    }

    #[test]
    fn dir_entry_names_filters_dotfiles() {
        let entries = vec![
            make_dir_entry(".hidden", false),
            make_dir_entry("visible.md", false),
        ];
        let names = dir_entry_names(&entries);
        assert_eq!(names, vec!["visible.md"]);
    }

    #[test]
    fn dir_entry_names_appends_slash_to_dirs() {
        let entries = vec![
            make_dir_entry("subdir", true),
            make_dir_entry("file.md", false),
        ];
        let names = dir_entry_names(&entries);
        assert!(names.contains(&"subdir/".to_string()));
        assert!(names.contains(&"file.md".to_string()));
    }

    #[test]
    fn dir_entry_names_sorts_results() {
        let entries = vec![
            make_dir_entry("z.md", false),
            make_dir_entry("a.md", false),
            make_dir_entry("m.md", false),
        ];
        let names = dir_entry_names(&entries);
        assert_eq!(names, vec!["a.md", "m.md", "z.md"]);
    }

    // --- build_browse_result ---

    #[test]
    fn build_browse_result_separates_agents_and_skills() {
        let mut skill_dirs = HashMap::new();
        let skill_path = PathBuf::from("my-skill");
        skill_dirs.insert(skill_path.clone(), vec!["tool.md".to_string()]);

        let artifacts = vec![make_agent("alpha"), make_skill("my-skill")];
        let result = build_browse_result("test-source", &artifacts, &skill_dirs);

        assert_eq!(result.source_name, "test-source");
        assert_eq!(result.agents.len(), 1);
        assert_eq!(result.agents[0].name, "alpha");
        assert_eq!(result.skills.len(), 1);
        assert_eq!(result.skills[0].name, "my-skill");
        assert_eq!(result.skills[0].files, vec!["tool.md"]);
    }

    #[test]
    fn build_browse_result_empty_skill_dirs_gives_empty_files() {
        let artifacts = vec![make_skill("lonely-skill")];
        let result = build_browse_result("src", &artifacts, &HashMap::new());
        assert_eq!(result.skills[0].files, Vec::<String>::new());
    }

    // --- build_browse_skill ---

    #[test]
    fn build_browse_skill_populates_fields() {
        let artifact = make_skill("my-skill");
        let files = vec!["a.md".to_string(), "b.md".to_string()];
        let skill = build_browse_skill(&artifact, files.clone());
        assert_eq!(skill.name, "my-skill");
        assert_eq!(skill.version, None);
        assert_eq!(skill.deprecation_display, "");
        assert_eq!(skill.files, files);
    }

    #[test]
    fn build_browse_skill_includes_version_and_deprecation() {
        let artifact = Artifact {
            kind: ArtifactKind::Skill,
            name: "my-skill".to_string(),
            description: String::new(),
            path: PathBuf::from("my-skill"),
            version: Some("1.2.3".to_string()),
            deprecation: Some(Deprecation {
                reason: Some("Old".to_string()),
                replacement: Some("new-skill".to_string()),
            }),
        };
        let skill = build_browse_skill(&artifact, vec![]);
        assert_eq!(skill.version, Some("1.2.3".to_string()));
        assert!(skill.deprecation_display.contains("DEPRECATED"));
    }
}
