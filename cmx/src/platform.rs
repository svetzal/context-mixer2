use std::fmt;
use std::path::PathBuf;

/// The target AI coding assistant platform for artifact installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Platform {
    #[default]
    Claude,
    Copilot,
    Cursor,
    Windsurf,
    Gemini,
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Copilot => write!(f, "copilot"),
            Self::Cursor => write!(f, "cursor"),
            Self::Windsurf => write!(f, "windsurf"),
            Self::Gemini => write!(f, "gemini"),
        }
    }
}

impl Platform {
    /// The base directory for local (project-scoped) installations.
    ///
    /// This is a relative path from the project root.
    pub fn project_base(self) -> PathBuf {
        match self {
            Self::Claude => PathBuf::from(".claude"),
            Self::Copilot => PathBuf::from(".github"),
            Self::Cursor => PathBuf::from(".cursor"),
            Self::Windsurf => PathBuf::from(".windsurf"),
            Self::Gemini => PathBuf::from(".gemini"),
        }
    }

    /// The base directory for global (user-scoped) installations, relative to `$HOME`.
    pub fn user_base(self) -> PathBuf {
        match self {
            Self::Claude => PathBuf::from(".claude"),
            Self::Copilot => PathBuf::from(".copilot"),
            Self::Cursor => PathBuf::from(".cursor"),
            Self::Windsurf => PathBuf::from(".codeium").join("windsurf"),
            Self::Gemini => PathBuf::from(".gemini"),
        }
    }

    /// Slug used to construct platform-specific lock file names.
    ///
    /// Claude returns an empty string (lock file stays `cmx-lock.json` for
    /// backward compatibility). All other platforms return a non-empty slug.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::Gemini => "gemini",
        }
    }

    /// The directory name for this platform's plugin manifest (used by cmf).
    pub fn manifest_dir(self) -> &'static str {
        match self {
            Self::Claude => ".claude-plugin",
            Self::Copilot => ".copilot-plugin",
            Self::Cursor => ".cursor-plugin",
            Self::Windsurf => ".windsurf-plugin",
            Self::Gemini => ".gemini-plugin",
        }
    }

    /// All non-Claude platforms that receive generated manifests.
    pub fn targets() -> &'static [Platform] {
        &[Self::Copilot, Self::Cursor, Self::Windsurf, Self::Gemini]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_claude() {
        assert_eq!(Platform::default(), Platform::Claude);
    }

    #[test]
    fn display_produces_lowercase_names() {
        assert_eq!(Platform::Claude.to_string(), "claude");
        assert_eq!(Platform::Copilot.to_string(), "copilot");
        assert_eq!(Platform::Cursor.to_string(), "cursor");
        assert_eq!(Platform::Windsurf.to_string(), "windsurf");
        assert_eq!(Platform::Gemini.to_string(), "gemini");
    }

    #[test]
    fn manifest_dir_values() {
        assert_eq!(Platform::Claude.manifest_dir(), ".claude-plugin");
        assert_eq!(Platform::Copilot.manifest_dir(), ".copilot-plugin");
        assert_eq!(Platform::Cursor.manifest_dir(), ".cursor-plugin");
        assert_eq!(Platform::Windsurf.manifest_dir(), ".windsurf-plugin");
        assert_eq!(Platform::Gemini.manifest_dir(), ".gemini-plugin");
    }

    #[test]
    fn targets_includes_windsurf() {
        assert!(
            Platform::targets().contains(&Platform::Windsurf),
            "expected Windsurf in targets()"
        );
    }

    #[test]
    fn targets_excludes_claude() {
        assert!(
            !Platform::targets().contains(&Platform::Claude),
            "Claude should not be in targets()"
        );
    }

    #[test]
    fn project_base_values() {
        assert_eq!(Platform::Claude.project_base(), PathBuf::from(".claude"));
        assert_eq!(Platform::Copilot.project_base(), PathBuf::from(".github"));
        assert_eq!(Platform::Cursor.project_base(), PathBuf::from(".cursor"));
        assert_eq!(Platform::Windsurf.project_base(), PathBuf::from(".windsurf"));
        assert_eq!(Platform::Gemini.project_base(), PathBuf::from(".gemini"));
    }

    #[test]
    fn user_base_values() {
        assert_eq!(Platform::Claude.user_base(), PathBuf::from(".claude"));
        assert_eq!(Platform::Copilot.user_base(), PathBuf::from(".copilot"));
        assert_eq!(Platform::Cursor.user_base(), PathBuf::from(".cursor"));
        assert_eq!(Platform::Windsurf.user_base(), PathBuf::from(".codeium").join("windsurf"));
        assert_eq!(Platform::Gemini.user_base(), PathBuf::from(".gemini"));
    }

    #[test]
    fn slug_values() {
        assert_eq!(Platform::Claude.slug(), "");
        assert_eq!(Platform::Copilot.slug(), "copilot");
        assert_eq!(Platform::Cursor.slug(), "cursor");
        assert_eq!(Platform::Windsurf.slug(), "windsurf");
        assert_eq!(Platform::Gemini.slug(), "gemini");
    }

    #[test]
    fn value_enum_parses_all_variants() {
        use clap::ValueEnum;
        assert_eq!(Platform::from_str("claude", true).unwrap(), Platform::Claude);
        assert_eq!(Platform::from_str("copilot", true).unwrap(), Platform::Copilot);
        assert_eq!(Platform::from_str("cursor", true).unwrap(), Platform::Cursor);
        assert_eq!(Platform::from_str("windsurf", true).unwrap(), Platform::Windsurf);
        assert_eq!(Platform::from_str("gemini", true).unwrap(), Platform::Gemini);
    }
}
